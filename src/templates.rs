use crate::{
    db, metrics as m,
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
use chrono::{DateTime, Local, NaiveDateTime};
use futures_util::{Stream, StreamExt};
use std::{collections::HashMap, convert::Infallible};

/// the format for sending dates to the frontend
pub static DATEFMT: &str = "%Y-%m-%dT%H:%M";

/// parse a datetime string in DATEFMT format to a local DateTime
fn parse_local_datetime(s: &str) -> Result<DateTime<Local>, chrono::ParseError> {
    NaiveDateTime::parse_from_str(s, DATEFMT).map(|naive| naive.and_local_timezone(Local).unwrap())
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct Index {
    np: Option<NowPlaying>,
    pub tasks: Tasks,
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
            tasks: Tasks::new(tasks),
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
                    Some(s) => parse_local_datetime(s).unwrap_or_else(|_| Local::now()),
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
                if let Some(Ok(d)) = q.get("time").map(|s| parse_local_datetime(s)) {
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
    pub elapsed: Vec<String>,
    pub refr: u32,
}
impl Tasks {
    fn new(tasks: Vec<Task>) -> Self {
        let (elapsed, refr): (Vec<_>, Vec<_>) = tasks
            .iter()
            .map(filters::task_elapsed)
            .map(Result::unwrap)
            .unzip();

        let refr = refr.into_iter().filter(|n| *n > 0).min().unwrap_or(0);

        Self {
            tasks,
            elapsed,
            refr,
        }
    }
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let tasks = p.lock().await.list_tasks()?;
        Ok(Self::new(tasks))
    }
    pub async fn post(
        State(p): AppState,
        Form(f): Form<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, Response> {
        let res = Self::post_inner(p.clone(), f).await;
        if let Err(e) = res {
            error!("post task: {e:#?}");
            return Err((StatusCode::BAD_REQUEST, e.to_string()).into_response());
        }

        Tasks::get(State(p)).await
    }
    async fn post_inner(p: Player, mut f: HashMap<String, String>) -> anyhow::Result<()> {
        let Some(name) = f.remove("name") else {
            anyhow::bail!("Missing value `name`")
        };
        let Some(Some(priority)) = f.remove("priority").map(|s| {
            Some(match s.as_str() {
                "false" | "0" | "off" => false,
                "true" | "1" | "on" => true,
                _ => return None,
            })
        }) else {
            anyhow::bail!("Missing or invalid value `priority`")
        };
        let Some(file_name) = f.remove("file_name") else {
            anyhow::bail!("Missing value `file_name`")
        };

        let task: Task = match f.get("type").map(String::as_str).unwrap_or("now") {
            "now" => Task::Now {
                name,
                priority,
                file_name,
            },
            "scheduled" => {
                if name.trim().is_empty() {
                    anyhow::bail!("`name` can't be empty")
                };
                let Some(Ok(time)) = f.remove("time").map(|s| parse_local_datetime(&s)) else {
                    anyhow::bail!("Missing or invalid value `time`")
                };

                // check if scheduled task is in the future
                (time - Local::now())
                    .to_std()
                    .map_err(|_| anyhow::anyhow!("Date is in the past"))?;

                let task = Task::Scheduled {
                    name,
                    priority,
                    file_name,
                    time,
                };

                db::insert_task(&*p.conn.lock().await, &task).map_err(db::db_err)?;
                task
            }
            "recurring" => {
                if name.trim().is_empty() {
                    anyhow::bail!("`name` can't be empty")
                };
                let Some(Ok(n)) = f.remove("recurring-n").map(|s| s.parse::<usize>()) else {
                    anyhow::bail!("Missing value `recurring-n`")
                };

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
                    priority,
                    file_name,
                    time,
                };

                db::insert_task(&*p.conn.lock().await, &task).map_err(db::db_err)?;
                task
            }
            _ => anyhow::bail!("Invalid value for `type`"),
        };

        // record task creation metric
        m::record_task_created(task.get_type());

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

            // update file stats
            let conn = p.conn.clone();
            tokio::spawn(async move {
                db::update_file_stats(&*conn.lock().await);
            });

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

        // update file stats
        let conn = p.conn.clone();
        tokio::spawn(async move {
            db::update_file_stats(&*conn.lock().await);
        });

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
            let Ok(i): Result<usize, _> = k.replace("time-", "").parse() else {
                continue;
            };
            let Ok(t) = parse_local_datetime(v) else {
                continue;
            };
            if times.len() <= i {
                times.resize(i + 1, None);
            }
            times[i] = Some(t);
        }
    }
}

pub use filters::dur_human;
mod filters {
    use std::time::Duration;

    use askama::Result;
    use chrono::{DateTime, Local, NaiveTime};

    use crate::{player::NowPlaying, Task};

    pub fn datefmt(d: &DateTime<Local>) -> Result<String> {
        Ok(d.format(super::DATEFMT).to_string())
    }
    pub fn durfmt(d: Duration) -> Result<String> {
        let secs = d.as_secs() as u32;
        let Some(time) = NaiveTime::from_num_seconds_from_midnight_opt(secs, d.subsec_nanos())
        else {
            return Ok("L".to_string());
        };

        let fmt = if secs >= 60 * 60 { "%H:%M:%S" } else { "%M:%S" };
        Ok(time.format(fmt).to_string())
    }
    pub fn task_timefmt(task: &Task) -> Result<String> {
        let s = match task {
            Task::Now { .. } => return Ok("".into()),
            Task::Scheduled { time, .. } => datefmt(time)?.replace('T', " "),
            Task::Recurring { .. } => task.time_to_str().unwrap().replace(';', ", "),
        };
        Ok(s)
    }
    pub fn task_elapsed(task: &Task) -> Result<(String, u32)> {
        let next: chrono::Duration = match task {
            Task::Now { .. } => return Ok(("".into(), 0)),
            Task::Scheduled { time, .. } => *time - Local::now(),
            Task::Recurring { time: times, .. } => {
                let now = Local::now().time();
                times
                    .iter()
                    .map(|t| *t - now)
                    .map(|t| {
                        if t.num_milliseconds() < 0 {
                            t + chrono::Duration::days(1)
                        } else {
                            t
                        }
                    })
                    .min()
                    .unwrap()
            }
        };
        Ok(dur_human(&next))
    }

    static DICT_FUT: [&str; 7] = [
        "másodperc múlva",
        "perc múlva",
        "óra múlva",
        "nap múlva",
        "hét múlva",
        "hónap múlva",
        "év múlva",
    ];
    static DICT_PAST: [&str; 7] = [
        "másodperce",
        "perce",
        "órája",
        "napja",
        "hete",
        "hónapja",
        "éve",
    ];
    static DIV: [u32; 8] = [
        1,
        60,
        60 * 60,
        60 * 60 * 24,
        60 * 60 * 24 * 7,
        60 * 60 * 24 * 30,
        60 * 60 * 24 * 365,
        u32::MAX,
    ];
    pub fn dur_human(d: &chrono::Duration) -> (String, u32) {
        let secs = d.num_seconds();
        if secs == 0 {
            return ("most".to_string(), 1); // refetch in 1s to see it disappear
        }

        let arr = if secs >= 0 { &DICT_FUT } else { &DICT_PAST };
        let secs = secs.unsigned_abs() as u32;
        for (word, (max, div)) in arr.iter().zip(DIV.iter().skip(1).zip(DIV)) {
            if secs > *max {
                continue;
            }
            return (format!("{} {word}", secs / div), div);
        }
        unreachable!("u32::MAX")
    }

    pub fn has_len(np: &Option<NowPlaying>) -> Result<bool> {
        Ok(np.as_ref().is_some_and(|n| n.len.is_some()))
    }
}
