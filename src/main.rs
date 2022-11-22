mod db;
mod player;
mod scheduler;
mod server;

#[macro_use]
extern crate log;
use bytes::Bytes;
use chrono::{DateTime, NaiveTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::env;

const GIT_REF: &str = include_str!("../.git/refs/heads/main");

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "warp=info,csengo=debug");
    }
    pretty_env_logger::init();

    info!("csengo v{} - starting...", &GIT_REF[0..7]);

    // db setup
    let (conn, db_new) = db::init()?;

    // audio setup
    let stream = player::Player::new_stream();
    let s: &'static rodio::OutputStreamHandle = &Box::leak(Box::new(stream)).1;
    let player = player::Player::new(s, conn);

    if !db_new {
        let l = db::load(player.clone()).await?;
        info!("old tasks loaded from db: {l}")
    }

    // web server setup
    server::init(player).await
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
