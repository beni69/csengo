mod db;
mod scheduler;
mod server;

#[macro_use]
extern crate log;
use bytes::Bytes;
use chrono::{DateTime, NaiveTime, SecondsFormat, Utc};
use rodio::{
    source::{SineWave, Source},
    Decoder, OutputStream, OutputStreamHandle, Sink,
};
use serde::{Deserialize, Serialize};
use std::{env, io::Cursor, sync::Arc};
use tokio::time::Duration;

const GIT_REF: &str = include_str!("../.git/refs/heads/main");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "warp=info,csengo=debug");
    }
    pretty_env_logger::init();

    info!("csengo v{} - starting...", &GIT_REF[0..7]);

    // audio setup
    let (_stream, stream_handle): (OutputStream, OutputStreamHandle) = OutputStream::try_default()?;
    let sink = Arc::new(Sink::try_new(&stream_handle)?);

    // db setup
    let (conn, db_new) = db::init()?;
    if !db_new {
        let l = db::load(conn.clone(), sink.clone()).await?;
        info!("old tasks loaded from db: {l}")
    }

    // web server setup
    server::init(conn, sink).await
}

// === data structures ===
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
                    .map(|t| t.format(db::TIMEFMT).to_string())
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

// === playing sounds ===
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
    // taken from https://docs.rs/rodio
    // Add a dummy source for the sake of the example.
    let source = SineWave::new(880.0)
        .take_duration(Duration::from_secs_f32(1.0))
        .amplify(0.20);
    sink.append(source);
    // sink.sleep_until_end();
    // sink.detach();
}
