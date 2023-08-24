use crate::{
    db,
    player::{NowPlaying, Player},
    scheduler::schedule,
    server::{err_to_reply, AppState},
    File, Task,
};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    Form,
};
use chrono::{DateTime, Local, TimeZone};
use futures_util::{Stream, StreamExt};
use std::{collections::HashMap, convert::Infallible};

/// the format for sending dates to the frontend
pub static DATEFMT: &str = "%Y-%m-%dT%H:%M";

#[derive(Template)]
#[template(path = "index.html")]
pub struct Index {
    np: Option<NowPlaying>,
    pub tasks: Vec<Task>,
    pub files: Vec<String>,
    pub time: Time,
}
impl Index {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let mut lock = p.lock().await;
        let tasks = lock.list_tasks()?;
        let files = lock.list_files()?;
        drop(lock);

        let np = p.now_playing().as_ref().map(Clone::clone);

        Ok(Self {
            np,
            tasks,
            files,
            time: Time::default(),
        })
    }
}

#[derive(Template)]
#[template(path = "status.html")]
pub struct Status {
    np: Option<NowPlaying>,
}
impl Status {
    pub async fn get(State(p): AppState) -> impl IntoResponse {
        let np = p.now_playing().to_owned();
        Self { np }
    }

    pub async fn sse(State(p): AppState) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
        let stream = p.np_stream();
        let stream = stream.map(|np| {
            let s = Self { np }.render().unwrap();
            Ok(Event::default().data(s.replace('\n', " ")))
        });
        Sse::new(stream).keep_alive(KeepAlive::default())
    }

    pub async fn realtime(State(p): AppState) -> impl IntoResponse {
        let mut rx = p.np_realtime();
        rx.changed().await.unwrap();
        let np = rx.borrow().to_owned();
        let mut res = (Self { np }).into_response();
        res.headers_mut()
            .insert("HX-Trigger", HeaderValue::from_static("realtime"));

        res
    }
}

#[derive(Template)]
#[template(path = "datepicker.html")]
pub struct DatePicker {
    pub time: Time,
}
#[derive(Default)]
pub enum Time {
    #[default]
    Now,
    Scheduled(DateTime<Local>),
    Recurring(Vec<DateTime<Local>>),
}
impl Time {
    pub fn is_now(&self) -> bool {
        match self {
            Time::Now => true,
            Time::Scheduled(_) => false,
            Time::Recurring(_) => false,
        }
    }
    pub fn is_scheduled(&self) -> bool {
        match self {
            Time::Now => false,
            Time::Scheduled(_) => true,
            Time::Recurring(_) => false,
        }
    }
    pub fn is_recurring(&self) -> bool {
        match self {
            Time::Now => false,
            Time::Scheduled(_) => false,
            Time::Recurring(_) => true,
        }
    }
}
impl DatePicker {
    pub async fn get(
        Query(q): Query<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, Response> {
        let time = match q.get("type").map(String::as_str).unwrap_or("now") {
            "now" => Time::Now,
            "scheduled" => {
                // carry over first recurring date
                let t = match q.get("time-0") {
                    Some(s) => Local
                        .datetime_from_str(s, DATEFMT)
                        .unwrap_or_else(|_| Local::now()),
                    None => Local::now(),
                };
                Time::Scheduled(t)
            }
            "recurring" => {
                let n: usize = match q.get("recurring-n").map(|s| s.parse()) {
                    Some(Ok(n)) => n,
                    _ => 1,
                };

                let mut times: Vec<Option<DateTime<Local>>> = Vec::new();
                // carry over scheduled date
                if let Some(Ok(d)) = q.get("time").map(|s| Local.datetime_from_str(s, DATEFMT)) {
                    times.push(Some(d));
                }

                query_times(&q, &mut times);

                if times.iter().any(Option::is_none) {
                    Time::Recurring(vec![Local::now(); n])
                } else {
                    let mut times = times.into_iter().map(Option::unwrap).collect::<Vec<_>>();
                    times.resize(n, Local::now());
                    Time::Recurring(times)
                }
            }
            _ => return Err((StatusCode::BAD_REQUEST, "Invalid value for `type`").into_response()),
        };
        Ok(Self { time })
    }
}

#[derive(Template)]
#[template(path = "filepicker.html")]
pub struct FilePicker {
    pub files: Vec<String>,
}

#[derive(Template)]
#[template(path = "form.html")]
pub struct TaskForm {
    pub files: Vec<String>,
    pub time: Time,
}
impl TaskForm {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let files = p.lock().await.list_files()?;
        Ok(Self {
            files,
            time: Time::default(),
        })
    }
}

#[derive(Template)]
#[template(path = "tasks.html")]
pub struct Tasks {
    pub tasks: Vec<Task>,
}
impl Tasks {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let tasks = p.lock().await.list_tasks()?;
        Ok(Self { tasks })
    }
    pub async fn post(
        State(p): AppState,
        Form(f): Form<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, Response> {
        dbg!(&f);
        let res = Self::post_inner(p.clone(), f).await;
        if let Err(e) = res {
            error!("post task: {e:#?}");
            return Err((StatusCode::BAD_REQUEST, e.to_string()).into_response());
        }

        Tasks::get(State(p)).await
    }
    async fn post_inner(p: Player, mut f: HashMap<String, String>) -> anyhow::Result<()> {
        let Some(name) = f.remove("name") else {anyhow::bail!("Missing value `name`")};
        let Some(file_name) = f.remove("file_name") else {anyhow::bail!("Missing value `file_name`")};

        let task: Task = match f.get("type").map(String::as_str).unwrap_or("now") {
            "now" => Task::Now { name, file_name },
            "scheduled" => {
                let Some(Ok(time)) = f.remove("time").map(|s|Local.datetime_from_str(&s, DATEFMT)) else {anyhow::bail!("Missing or invalid value `time`")};

                // check if scheduled task is in the future
                (time - Local::now())
                    .to_std()
                    .map_err(|_| anyhow::anyhow!("Date is in the past"))?;

                let task = Task::Scheduled {
                    name,
                    file_name,
                    time,
                };

                db::insert_task(&*p.conn.lock().await, &task).map_err(db::db_err)?;
                task
            }
            "recurring" => {
                let Some(Ok(n)) = f.remove("recurring-n").map(|s|s.parse::<usize>()) else {anyhow::bail!("Missing value `recurring-n`")};

                let mut times = Vec::new();
                query_times(&f, &mut times);

                if times.iter().any(Option::is_none) {
                    anyhow::bail!("Invalid values for `time-{{n}}`")
                }
                let times = times.into_iter().map(Option::unwrap).collect::<Vec<_>>();

                if times.len() != n {
                    anyhow::bail!(
                        "Invalid values for `time-{{n}}`. Expected {n}, got {}",
                        times.len()
                    );
                }

                let time = times.iter().map(DateTime::time).collect();

                let task = Task::Recurring {
                    name,
                    file_name,
                    time,
                };

                db::insert_task(&*p.conn.lock().await, &task).map_err(db::db_err)?;
                task
            }
            _ => anyhow::bail!("Invalid value for `type`"),
        };

        schedule(task, p).await?;
        Ok(())
    }

    pub async fn delete(
        State(p): AppState,
        Path(name): Path<String>,
    ) -> Result<impl IntoResponse, Response> {
        match p.db_name(db::delete_task, &name).await {
            Ok(v) => {
                if v {
                    if let Err(e) = p.cancel(&name).await {
                        return Err(err_to_reply(
                            e,
                            &name,
                            "Failed to issue cancel",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        ));
                    }
                } else {
                    return Err((StatusCode::NOT_FOUND, "Task not found").into_response());
                }
            }
            Err(e) => {
                return Err(err_to_reply(
                    e.into(),
                    &name,
                    "Failed to delete task",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ))
            }
        };

        Tasks::get(State(p)).await
    }
}

#[derive(Template)]
#[template(path = "files.html")]
pub struct Files {
    pub files: Vec<String>,
}
impl Files {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let files = p.lock().await.list_files()?;
        Ok(Self { files })
    }
    pub async fn post(
        State(p): AppState,
        mut form: Multipart,
    ) -> Result<impl IntoResponse, Response> {
        while let Some(field) = form.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            if name != "file" {
                continue;
            };
            let fname = field.file_name().unwrap().to_string();
            let data = field.bytes().await.unwrap();

            if let Err(e) = db::insert_file(
                &*p.conn.lock().await,
                File {
                    name: fname.clone(),
                    data,
                },
            ) {
                error!("failed to save file: {fname}\n{e:#?}");
                return Err(err_to_reply(
                    e.into(),
                    &fname,
                    "Failed to save file",
                    StatusCode::INSUFFICIENT_STORAGE,
                ));
            }

            return updated_files(p).await;
        }
        Err((StatusCode::BAD_REQUEST, "error").into_response())
    }
    pub async fn delete(
        State(p): AppState,
        Path(fname): Path<String>,
    ) -> Result<impl IntoResponse, Response> {
        if let Err(e) = db::delete_file(&*p.conn.lock().await, &fname) {
            error!("failed to delete file: {fname}\n{e:#?}");
            return Err(err_to_reply(
                e.into(),
                &fname,
                "Failed to delete file",
                StatusCode::NOT_FOUND,
            ));
        }
        updated_files(p).await
    }
}

/// to be sent back when the files were mutated, as these depend on that data
async fn updated_files(p: Player) -> Result<impl IntoResponse, Response> {
    let files = p.lock().await.list_files()?;

    let file_list = Files {
        files: files.clone(),
    }
    .render()
    .map_err(|e| {
        err_to_reply(
            e.into(),
            "",
            "Failed to render",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    let form = FilePicker { files }.render().map_err(|e| {
        err_to_reply(
            e.into(),
            "",
            "Failed to render",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    Ok(file_list + "\n\n" + &form)
}

/// extract an array from query params formatted: time-{i}
fn query_times(q: &HashMap<String, String>, times: &mut Vec<Option<DateTime<Local>>>) {
    for (k, v) in q.iter() {
        if k.starts_with("time-") {
            let Ok(i): Result<usize, _> = k.replace("time-", "").parse() else {continue};
            let Ok(t) = Local.datetime_from_str(v, DATEFMT) else {continue};
            if times.len() <= i {
                times.resize(i + 1, None);
            }
            times[i] = Some(t);
        }
    }
}

mod filters {
    use chrono::{DateTime, Local};

    pub fn datefmt(d: &DateTime<Local>) -> askama::Result<String> {
        Ok(d.format(super::DATEFMT).to_string())
    }
}
