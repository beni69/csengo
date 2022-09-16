use chrono::{DateTime, Utc};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::env;
use warp::{log, reply::json, Filter};

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Frontend;

#[tokio::main]
async fn main() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "warp=info");
    }
    pretty_env_logger::init();
    let frontend = warp::get().and(warp_embed::embed(&Frontend));
    let api =
        warp::path("api").and(warp::path("tasks").and(warp::get().map(|| json(&list_tasks()))));

    warp::serve(frontend.or(api).with(log("warp")))
        .run(([0, 0, 0, 0], 8080))
        .await;
}

#[derive(Serialize)]
struct Task {
    name: String,
    now: bool,
    time: Option<DateTime<Utc>>,
}
fn list_tasks() -> Vec<Task> {
    vec![
        Task {
            name: "becsengo".into(),
            now: false,
            time: Some(Utc::now()),
        },
        Task {
            name: "kicsengo".into(),
            now: true,
            time: None,
        },
    ]
}
