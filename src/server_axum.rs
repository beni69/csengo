use crate::{player::Player, templates};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Router,
};
use rust_embed::RustEmbed;
use std::{env::var, net::IpAddr, sync::Arc};

pub type AppState = State<Arc<Player>>;

#[derive(RustEmbed)]
#[folder = "static"]
pub struct Static;

pub async fn init(p: Arc<Player>) -> ! {
    let app = Router::new()
        .route("/", get(templates::Index::get))
        .route("/static/*path", get(static_handler))
        .nest(
            "/htmx",
            Router::new()
                .route("/form", get(templates::TaskForm::get))
                .route("/task", get(templates::Tasks::get))
                .route("/task", post(templates::Tasks::post))
                .route("/task/:id", delete(templates::Tasks::delete))
                .route("/file", get(templates::Files::get))
                .route("/file", post(templates::Files::post))
                .route("/file/:fname", delete(templates::Files::delete))
                .route("/datepicker", get(templates::DatePicker::get)),
        )
        // .nest("/api", Router::new())
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

async fn static_handler(Path(path): Path<String>) -> impl IntoResponse {
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
    e: impl std::error::Error,
    name: &str,
    msg: &'static str,
    status: StatusCode,
) -> Response {
    error!("{name}: {msg}\n{e:#?}");
    (status, msg).into_response()
}
