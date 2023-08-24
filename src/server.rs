use crate::{db, player::Player, scheduler::schedule, templates, Task};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::{env::var, net::IpAddr};

pub type AppState = State<Player>;

#[derive(RustEmbed)]
#[folder = "static"]
pub struct Static;

pub async fn init(p: Player) -> ! {
    let app = Router::new()
        .route("/", get(templates::Index::get))
        .route("/static/*path", get(static_handler))
        .nest(
            "/htmx",
            Router::new()
                .route("/status", get(templates::Status::get))
                .route("/status/sse", get(templates::Status::sse))
                .route("/status/realtime", get(templates::Status::realtime))
                .route("/form", get(templates::TaskForm::get))
                .route("/datepicker", get(templates::DatePicker::get))
                .route("/task", get(templates::Tasks::get))
                .route("/task", post(templates::Tasks::post))
                .route("/task/:id", delete(templates::Tasks::delete))
                .route("/file", get(templates::Files::get))
                .route(
                    "/file",
                    post(templates::Files::post)
                        .layer(DefaultBodyLimit::max(1024usize.pow(2) * 100)), // 100M
                )
                .route("/file/:fname", delete(templates::Files::delete)),
        )
        .nest(
            "/api",
            Router::new()
                .route("/stop", any(api_stop))
                .route("/playtest", post(api_playtest))
                .route("/export", get(api_export))
                .route("/import", get(api_import))
                .route("/file/:fname", get(api_download)),
        )
        .with_state(p);

    axum::Server::bind(
        &(
            var("HOST")
                .map(|s| s.parse::<IpAddr>().expect("invalid $HOST"))
                .unwrap_or_else(|_| [0, 0, 0, 0].into()),
            var("PORT")
                .map(|s| s.parse().expect("invalid $PORT"))
                .unwrap_or(8080u16),
        )
            .into(),
    )
    .serve(app.into_make_service())
    .await
    .unwrap();

    unreachable!()
}

async fn api_stop(State(p): AppState) -> StatusCode {
    p.stop();
    info!("STOP");
    StatusCode::NO_CONTENT
}
async fn api_playtest(State(p): AppState) -> StatusCode {
    p.playtest();
    StatusCode::NO_CONTENT
}

async fn api_export(State(p): AppState) -> Result<Json<Vec<Task>>, Response> {
    let tasks = p.lock().await.list_tasks()?;
    Ok(Json(tasks))
}
async fn api_import(
    State(p): AppState,
    Json(tasks): Json<Vec<Task>>,
) -> Result<impl IntoResponse, Response> {
    let lock = p.lock().await;
    let len = tasks.len();
    let mut n = 0;
    for task in tasks {
        if db::get_task(&lock.lock, task.get_name()).is_err() {
            continue;
        };

        db::insert_task(&lock.lock, &task).unwrap();
        schedule(task, p.clone())
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()).into_response())?;
        n += 1;
    }
    let msg = format!(
        "Task import complete: {len} total, {n} new, {} skipped",
        len - n
    );
    info!("{msg}");

    Ok(msg)
}
async fn api_download(
    State(p): AppState,
    Path(fname): Path<String>,
) -> Result<Response, StatusCode> {
    let file = db::get_file(&p.lock().await.lock, &fname).map_err(|_| (StatusCode::NOT_FOUND))?;
    let mime = mime_guess::from_path(&fname).first_or_octet_stream();
    Ok(([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response())
}

async fn static_handler(Path(path): Path<String>) -> Response {
    let path = path.replacen("static/", "", 1);
    match Static::get(path.as_str()) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn err_to_reply(
    e: anyhow::Error,
    name: &str,
    msg: &'static str,
    status: StatusCode,
) -> Response {
    error!("{name}: {msg}\n{e:#?}");
    (status, msg).into_response()
}
