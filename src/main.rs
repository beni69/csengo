mod db;
mod mail;
mod metrics;
mod player;
mod scheduler;
mod server;
mod sink;
mod templates;

#[macro_use]
extern crate log;
use bytes::Bytes;
use chrono::{DateTime, Local, NaiveTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use std::env;

include!(concat!(env!("OUT_DIR"), "/const_gen.rs"));

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "warp=info,csengo=debug");
    }
    pretty_env_logger::init();

    info!("csengo starting...");
    info!("version {}", GIT_REF);

    // metrics setup
    let metrics_handle = metrics::init();
    info!("metrics initialized");

    // db setup
    let (conn, db_new) = db::init()?;

    // Initialize file stats from existing database
    db::update_file_stats(&*conn.lock().await);

    // audio setup
    let (controller, np_rx) = sink::Controller::init();
    let player = player::Player::new(controller, np_rx, conn, metrics_handle);

    if !db_new {
        let l = db::load(player.clone()).await?;
        info!("old tasks loaded from db: {l}")
    }

    // warn if the vars aren't set
    mail::get_vars();

    // web server setup
    server::init(player).await
}

// === data structures ===
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Task {
    Now {
        name: String,
        priority: bool,
        file_name: String,
    },
    Scheduled {
        name: String,
        file_name: String,
        priority: bool,
        time: DateTime<Local>,
    },
    Recurring {
        name: String,
        file_name: String,
        priority: bool,
        time: Vec<NaiveTime>,
    },
}
impl Task {
    pub fn is_now(&self) -> bool {
        match self {
            Task::Now { .. } => true,
            Task::Scheduled { .. } => false,
            Task::Recurring { .. } => false,
        }
    }
    pub fn get_type(&self) -> &str {
        match self {
            Task::Now { .. } => "now",
            Task::Scheduled { .. } => "scheduled",
            Task::Recurring { .. } => "recurring",
        }
    }
    pub fn get_name(&self) -> &String {
        match self {
            Task::Now { name, .. } => name,
            Task::Scheduled { name, .. } => name,
            Task::Recurring { name, .. } => name,
        }
    }
    pub fn get_priority(&self) -> bool {
        match *self {
            Task::Now { priority, .. } => priority,
            Task::Scheduled { priority, .. } => priority,
            Task::Recurring { priority, .. } => priority,
        }
    }
    pub fn get_file_name(&self) -> &String {
        match self {
            Task::Now { file_name, .. } => file_name,
            Task::Scheduled { file_name, .. } => file_name,
            Task::Recurring { file_name, .. } => file_name,
        }
    }
    pub fn time_to_str(&self) -> Option<String> {
        match self {
            Task::Now { .. } => None,
            Task::Scheduled { time, .. } => Some(time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            Task::Recurring { time, .. } => Some(
                time.iter()
                    .map(|t| t.format(db::TIMEFMT).to_string())
                    .collect::<Vec<String>>()
                    .join(";"),
            ),
        }
    }
}

#[derive(Debug)]
pub struct File {
    name: String,
    data: Bytes,
}
