#[macro_use]
extern crate log;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use rodio::{
    source::{SineWave, Source},
    Decoder, OutputStream, OutputStreamHandle, Sink,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{
    env,
    io::{BufReader, Cursor},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs::File, io::AsyncWriteExt, time::Duration};
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
    let (_stream, stream_handle): (OutputStream, OutputStreamHandle) =
        OutputStream::try_default().unwrap();
    let sink = Arc::new(Sink::try_new(&stream_handle).unwrap());
    let sink_ref1 = sink.clone();

    // db setup
    // let conn = rusqlite::Connection::open("./csengo.db")?;
    // conn.execute_batch(
    //     "CREATE TABLE IF NOT EXISTS task_s (
    //         name      TEXT PRIMARY KEY,
    //         file_name TEXT NOT NULL,
    //         time      TEXT
    //     );
    //     CREATE TABLE IF NOT EXISTS task_r (
    //         name      TEXT PRIMARY KEY,
    //         file_name TEXT NOT NULL,
    //         time      TEXT
    //     );
    //     CREATE TABLE IF NOT EXISTS files (
    //         name      TEXT PRIMARY KEY,
    //         data      BLOB
    //     );",
    // )?;

    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite:csengo.db")
        .await?;

    let tasks = warp::path("tasks")
        .and(warp::path::end())
        .and(warp::get())
        .map(list_tasks);
    let files = warp::path("files")
        .and(warp::path::end())
        .and(warp::get())
        .map(list_files);
    let post = warp::path::end()
        .and(warp::post())
        .and(warp::body::json())
        .map(move |x| (x, sink.clone()))
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

#[derive(Debug, Deserialize)]
struct Post {
    file: Option<Bytes>,
    task: Task,
}

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
        time: Vec<DateTime<Utc>>,
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
}

fn list_tasks() -> impl warp::Reply {
    let tasks: Vec<Task> = vec![
        Task::Now {
            name: "becsengo".into(),
            file_name: "becsengo.mp3".into(),
        },
        Task::Scheduled {
            name: "kicsengo".into(),
            time: Utc::now(),
            file_name: "kicsengo.mp3".into(),
        },
    ];
    json(&tasks)
}
fn list_files() -> impl warp::Reply {
    let files = vec!["becsengo.mp3", "kicsengo.mp3"];
    json(&files)
}

async fn post_task((req, sink): (Post, Arc<Sink>)) -> Box<dyn warp::Reply> {
    // a special case for one-off upload instant plays: don't save the file, just play it
    if req.task.is_now() {
        if let Some(file) = req.file {
            let file = base64::decode(file).expect("invalid base64").into();

            info!(
                "playing {}: {}",
                req.task.get_name(),
                req.task.get_file_name()
            );
            if let Err(e) = play_buf(file, sink.as_ref()) {
                error!("playing failed: {}\n{e:#?}", req.task.get_name());
                return Box::new(warp::reply::with_status(
                    "failed to play file",
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }

            // save_file(&req.task.get_file_name(), &mut file)
            //     .await
            //     .unwrap();
            // play_file(&req.task.get_file_name(), sink.as_ref())
            //     .await
            //     .unwrap();

            return Box::new("OK");
        }
    }

    if let Some(file) = req.file {
        let mut file = base64::decode(file).expect("invalid base64").into();
        if let Err(e) = save_file(req.task.get_file_name(), &mut file).await {
            error!("failed to save file: {}\n{e:#?}", req.task.get_file_name());
            return Box::new(warp::reply::with_status(
                "failed to save file",
                StatusCode::INSUFFICIENT_STORAGE,
            ));
        };
    }

    match req.task {
        Task::Now {
            name: _,
            file_name: _,
        } => unreachable!(),
        Task::Scheduled {
            name,
            file_name,
            time,
        } => {
            let diff = match (time - Utc::now()).to_std() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}: invalid time: {e:#?}", name);
                    return Box::new(warp::reply::with_status(
                        "time must be in the future",
                        StatusCode::BAD_REQUEST,
                    ));
                }
            };

            tokio::task::spawn(async move {
                debug!("{}: waiting {}s", name, diff.as_secs());
                tokio::time::sleep(diff).await;
                play_file(&file_name, sink.as_ref()).await.unwrap();
            });
        }
        Task::Recurring {
            name,
            file_name,
            time,
        } => {
            //
            todo!()
        }
    }

    Box::new("OK")
}
async fn save_file(name: &str, file: &mut Bytes) -> tokio::io::Result<()> {
    let mut f = File::create(PathBuf::from("./tmp").join(name)).await?;
    f.write_all_buf(file).await?;
    Ok(())
}
fn play_buf(buf: Bytes, sink: &Sink) -> anyhow::Result<()> {
    let src = Decoder::new(Cursor::new(buf))?;
    Ok(sink.append(src))
}
async fn play_file(fname: &str, sink: &Sink) -> anyhow::Result<()> {
    let file = File::open(fname).await?.into_std().await;
    let src = Decoder::new(BufReader::new(file))?;
    debug!("playing: {fname}");
    sink.append(src);
    Ok(())
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
