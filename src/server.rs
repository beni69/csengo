use crate::{
    db,
    player::{NowPlaying, Player},
    scheduler::{schedule_recurring, schedule_task},
    File, Task,
};
use bytes::Bytes;
use chrono::Utc;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{env::var, net::IpAddr, sync::Arc};
use warp::{hyper::StatusCode, reply::json, Filter};

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Frontend;

pub(crate) async fn init(p: Arc<Player>) -> ! {
    let status = warp::path("status")
        .and(warp::path::end())
        .and(warp::get())
        .map({
            let p = p.clone();
            move || p.clone()
        })
        .then(get_status);

    let realtime = warp::path("status")
        .and(warp::path("realtime"))
        .and(warp::path::end())
        .and(warp::get())
        .map({
            let p = p.clone();
            move || p.clone()
        })
        .then(get_realtime);

    let post = warp::path::end()
        .and(warp::post())
        .and(warp::body::json())
        .map({
            let p = p.clone();
            move |x| (x, p.clone())
        })
        .then(post_task);

    let delete = warp::path!("task" / String)
        .and(warp::path::end())
        .and(warp::delete())
        .map({
            let p = p.clone();
            move |s| (s, p.clone())
        })
        .then(delete_task);

    let stop = warp::path("stop")
        .and(warp::path::end())
        .and(warp::get().or(warp::post()))
        .map({
            let p = p.clone();
            move |_| {
                p.stop();
                "OK"
            }
        });

    let playtest = warp::path("playtest")
        .and(warp::path::end())
        .and(warp::post())
        .map({
            let p = p.clone();
            move || {
                p.playtest();
                "OK"
            }
        });

    let api = warp::path("api").and(
        status
            .or(realtime)
            .or(post)
            .or(delete)
            .or(stop)
            .or(playtest),
    );

    let frontend = warp::get().and(warp_embed::embed(&Frontend));
    let server = frontend.or(api).with(warp::log("server"));

    #[cfg(debug_assertions)]
    let server = server.with(warp::cors().allow_any_origin());

    warp::serve(server)
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

async fn get_status(p: Arc<Player>) -> Box<dyn warp::Reply> {
    let conn = &*p.conn.lock().await;
    let tasks = match db::list_tasks(conn) {
        Ok(x) => x,
        Err(e) => {
            return Box::new(err_to_reply(
                e,
                "list tasks",
                "failed to get tasks",
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    };
    let files = match db::list_files(conn) {
        Ok(x) => x,
        Err(e) => {
            return Box::new(err_to_reply(
                e,
                "list files",
                "failed to get files",
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    };
    Box::new(json(&Status {
        tasks,
        files,
        playing: &p.now_playing(),
    }))
}

async fn get_realtime(p: Arc<Player>) -> Box<dyn warp::Reply> {
    let mut rx = p.np_realtime();
    if let Err(e) = rx.changed().await {
        return Box::new(err_to_reply(
            e,
            "rt_recv",
            "failed to read realtime status",
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    };
    let np = rx.borrow();
    Box::new(json(&*np))
}

#[derive(Debug, Serialize)]
struct Status<'a> {
    tasks: Vec<Task>,
    files: Vec<String>,
    playing: &'a Option<NowPlaying>,
}

#[derive(Debug, Deserialize)]
struct Post {
    file: Option<Bytes>,
    task: Task,
}

async fn post_task((req, player): (Post, Arc<Player>)) -> Box<dyn warp::Reply> {
    // a special case for one-off upload instant plays: don't save the file, just play it
    if req.task.is_now() {
        if let Some(file) = req.file {
            let file = base64::decode(file).expect("invalid base64").into();

            info!(
                "playing {}: {}",
                req.task.get_name(),
                req.task.get_file_name()
            );
            if let Err(e) = player.play_buf(file, req.task.get_file_name()) {
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
            &*player.conn.lock().await,
            File {
                name: req.task.get_file_name().clone(),
                data: file,
            },
        ) {
            error!("failed to save file: {}\n{e:#?}", req.task.get_file_name());
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
            if let Err(e) = player.play_file(&file_name).await {
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
                    return Box::new(err_to_reply(
                        e,
                        name,
                        "invalid time",
                        StatusCode::BAD_REQUEST,
                    ));
                }
            };

            if let Err(e) = db::insert_task(&*player.conn.lock().await, &req.task) {
                error!("{name}: db insert failed\n{e:#?}");
                return Box::new(err_to_reply(
                    e,
                    name,
                    "failed to save task",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            };

            if let Err(e) = schedule_task(name.clone(), file_name.clone(), *time, player) {
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
            if let Err(e) = db::insert_task(&*player.conn.lock().await, &req.task) {
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
                schedule_recurring(name.clone(), file_name.clone(), time.clone(), player)
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

async fn delete_task((name, player): (String, Arc<Player>)) -> Box<dyn warp::Reply> {
    match player.db_name(db::delete_task, &name).await {
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
