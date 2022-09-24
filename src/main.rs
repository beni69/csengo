mod db;

#[macro_use]
extern crate log;
use bytes::Bytes;
use chrono::{DateTime, NaiveTime, SecondsFormat, Utc};
use rodio::{
    source::{SineWave, Source},
    Decoder, OutputStream, OutputStreamHandle, Sink,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{env, io::Cursor, sync::Arc};
use tokio::time::{interval_at, Duration, Instant, MissedTickBehavior};
use warp::{hyper::StatusCode, reply::json, Filter};

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Frontend;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "warp=info,csengo=debug");
    }
    pretty_env_logger::init();

    // audio setup
    let (_stream, stream_handle): (OutputStream, OutputStreamHandle) = OutputStream::try_default()?;
    let sink = Arc::new(Sink::try_new(&stream_handle)?);
    let sink_ref1 = sink.clone();

    // db setup
    let (conn, db_new) = db::init()?;
    if !db_new {
        let l = db::load(conn.clone(), sink.clone()).await?;
        info!("old tasks loaded from db: {l}")
    }

    // web server
    let conn_ref = conn.clone();
    let tasks = warp::path("tasks")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || conn_ref.clone())
        .then(db::list_tasks)
        .map(wrap_db("tasks"));

    let conn_ref = conn.clone();
    let files = warp::path("files")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || conn_ref.clone())
        .then(db::list_files)
        .map(wrap_db("files"));

    let post = warp::path::end()
        .and(warp::post())
        .and(warp::body::json())
        .map(move |x| (x, conn.clone(), sink.clone()))
        .then(post_task);

    let playtest = warp::path("playtest")
        .and(warp::path::end())
        .and(warp::post())
        .map(move || {
            playtest(&sink_ref1);
            "done"
        });
    let api = warp::path("api").and(tasks.or(files).or(post).or(playtest));

    let frontend = warp::get().and(warp_embed::embed(&Frontend));

    warp::serve(frontend.or(api).with(warp::log("warp")))
        .run(([0, 0, 0, 0], 8080))
        .await;

    unreachable!();
}

fn wrap_db<T: Serialize>(
    name: &'static str,
) -> impl Fn(rusqlite::Result<T>) -> Box<dyn warp::Reply> + Clone {
    move |x| match x {
        Ok(v) => Box::new(json(&v)),
        Err(e) => {
            error!("(list {name}): db read failed\n{e:#?}");
            Box::new(warp::reply::with_status(
                format!("failed to get {name}"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
struct Post {
    file: Option<Bytes>,
    task: Task,
}

static TIMEFMT: &str = "%H:%M";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
enum Task {
    Now {
        name: String,
        file_name: String,
    },
    Scheduled {
        name: String,
        file_name: String,
        time: DateTime<Utc>,
    },
    Recurring {
        name: String,
        file_name: String,
        // time: Vec<DateTime<Utc>>,
        time: Vec<NaiveTime>,
    },
}
impl Task {
    pub fn is_now(&self) -> bool {
        match self {
            Task::Now {
                name: _,
                file_name: _,
            } => true,
            Task::Scheduled {
                name: _,
                file_name: _,
                time: _,
            } => false,
            Task::Recurring {
                name: _,
                file_name: _,
                time: _,
            } => false,
        }
    }
    pub fn get_type(&self) -> &str {
        match self {
            Task::Now {
                name: _,
                file_name: _,
            } => "now",
            Task::Scheduled {
                name: _,
                file_name: _,
                time: _,
            } => "scheduled",
            Task::Recurring {
                name: _,
                file_name: _,
                time: _,
            } => "recurring",
        }
    }
    pub fn get_name(&self) -> &String {
        match self {
            Task::Now { name, file_name: _ } => name,
            Task::Scheduled {
                name,
                file_name: _,
                time: _,
            } => name,
            Task::Recurring {
                name,
                file_name: _,
                time: _,
            } => name,
        }
    }
    pub fn get_file_name(&self) -> &String {
        match self {
            Task::Now { name: _, file_name } => file_name,
            Task::Scheduled {
                name: _,
                file_name,
                time: _,
            } => file_name,
            Task::Recurring {
                name: _,
                file_name,
                time: _,
            } => file_name,
        }
    }
    pub fn time_to_str(&self) -> Option<String> {
        match self {
            Task::Now {
                name: _,
                file_name: _,
            } => None,
            Task::Scheduled {
                name: _,
                file_name: _,
                time,
            } => Some(time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            Task::Recurring {
                name: _,
                file_name: _,
                time,
            } => Some(
                time.iter()
                    // .map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true))
                    .map(|t| t.format(TIMEFMT).to_string())
                    .collect::<Vec<String>>()
                    .join(";"),
            ),
        }
    }
}

#[derive(Debug)]
struct File {
    name: String,
    data: Bytes,
}

async fn post_task((req, conn, sink): (Post, db::Db, Arc<Sink>)) -> Box<dyn warp::Reply> {
    // a special case for one-off upload instant plays: don't save the file, just play it
    if req.task.is_now() {
        if let Some(file) = req.file {
            let file = base64::decode(file).expect("invalid base64").into();

            info!(
                "playing {}: {}",
                req.task.get_name(),
                req.task.get_file_name()
            );
            if let Err(e) = play_buf(file, &sink) {
                error!("playing failed: {}\n{e:#?}", req.task.get_name());
                return Box::new(warp::reply::with_status(
                    "failed to play file",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }

            return Box::new("OK");
        }
    }

    if let Some(file) = req.file {
        let file = base64::decode(file).expect("invalid base64").into();
        if let Err(e) = db::insert_file(
            &*conn.lock().await,
            File {
                name: req.task.get_file_name().clone(),
                data: file,
            },
        ) {
            // error!("failed to save file: {}\n{e:#?}", req.task.get_file_name()); return Box::new(warp::reply::with_status(     "failed to save file",     StatusCode::INSUFFICIENT_STORAGE, ));
            return Box::new(err_to_reply(
                e,
                req.task.get_file_name(),
                "failed to save file",
                StatusCode::INSUFFICIENT_STORAGE,
            ));
        };
    }

    match req.task {
        Task::Now { name, file_name } => {
            if let Err(e) = play_file(&file_name, &conn, &sink).await {
                return Box::new(err_to_reply(
                    e.root_cause(),
                    &name,
                    "playing failed",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            };
        }
        Task::Scheduled {
            ref name,
            ref file_name,
            ref time,
        } => {
            match (*time - Utc::now()).to_std() {
                Ok(_) => (),
                Err(e) => {
                    // error!("{}: invalid time: {e:#?}", name); return Box::new(warp::reply::with_status(     "time must be in the future",     StatusCode::BAD_REQUEST, ));
                    return Box::new(err_to_reply(
                        e,
                        name,
                        "invalid time",
                        StatusCode::BAD_REQUEST,
                    ));
                }
            };

            if let Err(e) = db::insert_task(&*conn.clone().lock().await, &req.task) {
                // error!("{name}: db insert failed\n{e:#?}"); return Box::new(warp::reply::with_status(     "failed to save task",     StatusCode::INTERNAL_SERVER_ERROR, ));
                return Box::new(err_to_reply(
                    e,
                    name,
                    "failed to save task",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            };

            if let Err(e) = schedule_task(name.clone(), file_name.clone(), time, conn, sink) {
                return Box::new(err_to_reply(
                    e.root_cause(),
                    name,
                    "failed to schedule task",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            };
        }
        Task::Recurring {
            name,
            file_name,
            time,
        } => {
            if let Err(e) = schedule_recurring(name.clone(), file_name, time, conn, sink) {
                return Box::new(err_to_reply(
                    e.root_cause(),
                    &name,
                    "",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        }
    }

    Box::new("OK")
}

fn err_to_reply(
    e: impl std::error::Error,
    name: &str,
    msg: &'static str,
    status: StatusCode,
) -> warp::reply::WithStatus<&'static str> {
    error!("{name}: {msg}\n{e:#?}");
    warp::reply::with_status(msg, status)
}

fn schedule_task(
    name: String,
    file_name: String,
    time: &DateTime<Utc>,
    conn: db::Db,
    sink: Arc<Sink>,
) -> anyhow::Result<()> {
    let diff = (*time - Utc::now()).to_std()?;
    tokio::task::spawn(async move {
        debug!("{}: waiting {}s", name, diff.as_secs());
        tokio::time::sleep(diff).await;
        if let Err(e) = play_file(&file_name, &conn, &sink).await {
            error!("error while playing {}:\n{e:#?}", &file_name);
        }
        if let Err(e) = db::delete_task(&*conn.lock().await, &name) {
            error!("{name}: failed to delete task after scheduled play\n{e:#?}");
        };
    });

    Ok(())
}
fn schedule_recurring(
    name: String,
    file_name: String,
    times: Vec<NaiveTime>,
    conn: db::Db,
    sink: Arc<Sink>,
) -> anyhow::Result<()> {
    for time in times {
        let diff: Duration = match (time - Utc::now().time()).to_std() {
            Ok(d) => d,
            Err(_) => ((Utc::today()
                .and_time(time)
                .expect("datetime construction failed")
                + chrono::Duration::days(1))
                - (Utc::now()))
            .to_std()
            .expect("time went backwards"),
        };
        let start: Instant = Instant::now() + diff;
        let name = name.clone();
        let fname = file_name.clone();
        let conn = conn.clone();
        let sink = sink.clone();
        tokio::task::spawn(async move {
            let mut interval = interval_at(start, Duration::from_secs(24 * 60 * 60));
            interval.set_missed_tick_behavior(MissedTickBehavior::Burst);

            loop {
                interval.tick().await;
                if let Err(e) = play_file(&fname, &conn, &sink).await {
                    error!("{name}: recurring play failed\n{e:#?}");
                } else {
                    debug!("{name}: added to queue, going back to sleep");
                }
            }
        });
    }

    Ok(())
}

fn play_buf(buf: Bytes, sink: &Sink) -> anyhow::Result<()> {
    let src = Decoder::new(Cursor::new(buf))?;
    sink.append(src);
    Ok(())
}
async fn play_file(fname: &str, conn: &db::Db, sink: &Sink) -> anyhow::Result<()> {
    let file = db::get_file(&*conn.lock().await, fname)?;
    debug!("playing: {fname}");
    play_buf(file.data, sink)
}

fn playtest(sink: &Sink) {
    // Add a dummy source for the sake of the example.
    let source = SineWave::new(440.0)
        .take_duration(Duration::from_secs_f32(1.0))
        .amplify(0.20);
    sink.append(source);
    // sink.sleep_until_end();
    // sink.detach();
}
