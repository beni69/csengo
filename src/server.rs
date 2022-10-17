use crate::{
    db::{self, Db},
    play_buf, play_file, playtest,
    scheduler::{schedule_recurring, schedule_task},
    File, Task,
};
use bytes::Bytes;
use chrono::Utc;
use rodio::Sink;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{env::var, net::IpAddr, sync::Arc};
use warp::{hyper::StatusCode, reply::json, Filter};

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Frontend;

pub(crate) async fn init(conn: Db, sink: Arc<Sink>) -> ! {
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

    let conn_ref = conn.clone();
    let sink_ref = sink.clone();
    let post = warp::path::end()
        .and(warp::post())
        .and(warp::body::json())
        .map(move |x| (x, conn_ref.clone(), sink_ref.clone()))
        .then(post_task);

    let conn_ref = conn.clone();
    let delete = warp::path!("task" / String)
        .and(warp::path::end())
        .and(warp::delete())
        .map(move |s| (s, conn_ref.clone()))
        .then(delete_task);

    let sink_ref = sink.clone();
    let playtest = warp::path("playtest")
        .and(warp::path::end())
        .and(warp::post())
        .map(move || {
            playtest(&sink_ref);
            "done"
        });

    let api = warp::path("api").and(tasks.or(files).or(post).or(delete).or(playtest));

    let frontend = warp::get().and(warp_embed::embed(&Frontend));

    warp::serve(frontend.or(api).with(warp::log("server")))
        .run((
            var("HOST")
                .map(|s| s.parse::<IpAddr>().expect("invalid $HOST"))
                .unwrap_or_else(|_| [0, 0, 0, 0].into()),
            var("PORT")
                .map(|s| s.parse().expect("invalid $PORT"))
                .unwrap_or(8080u16),
        ))
        .await;

    unreachable!()
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
            error!("failed to save file: {}\n{e:#?}", req.task.get_file_name());
            // return Box::new(warp::reply::with_status(     "failed to save file",     StatusCode::INSUFFICIENT_STORAGE, ));
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
                    error!("{}: invalid time: {e:#?}", name);
                    // return Box::new(warp::reply::with_status(     "time must be in the future",     StatusCode::BAD_REQUEST, ));
                    return Box::new(err_to_reply(
                        e,
                        name,
                        "invalid time",
                        StatusCode::BAD_REQUEST,
                    ));
                }
            };

            if let Err(e) = db::insert_task(&*conn.clone().lock().await, &req.task) {
                error!("{name}: db insert failed\n{e:#?}");
                // return Box::new(warp::reply::with_status(     "failed to save task",     StatusCode::INTERNAL_SERVER_ERROR, ));
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
            ref name,
            ref file_name,
            ref time,
        } => {
            if let Err(e) = db::insert_task(&*conn.clone().lock().await, &req.task) {
                error!("{name}: db insert failed\n{e:#?}");
                // return Box::new(warp::reply::with_status(     "failed to save task",     StatusCode::INTERNAL_SERVER_ERROR, ));
                return Box::new(err_to_reply(
                    e,
                    name,
                    "failed to save task",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            };

            if let Err(e) =
                schedule_recurring(name.clone(), file_name.clone(), time.clone(), conn, sink)
            {
                return Box::new(err_to_reply(
                    e.root_cause(),
                    name,
                    "",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        }
    }

    Box::new("OK")
}

async fn delete_task((name, conn): (String, db::Db)) -> Box<dyn warp::Reply> {
    match db::delete_task(&*conn.lock().await, &name) {
        Ok(v) => {
            if v {
                Box::new("OK")
            } else {
                Box::new(warp::reply::with_status(
                    "task not found",
                    StatusCode::NOT_FOUND,
                ))
            }
        }
        Err(e) => {
            error!("{name}: failed to delete task");
            Box::new(err_to_reply(
                e,
                &name,
                "msg",
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
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
