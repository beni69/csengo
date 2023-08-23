use crate::{
    db,
    player::Player,
    server_axum::{err_to_reply, AppState},
    File, Task,
};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, NaiveTime, Utc};
use std::{collections::HashMap, sync::Arc};

#[derive(Template)]
#[template(path = "index.html")]
pub struct Index {
    pub tasks: Vec<Task>,
    pub files: Vec<String>,
    pub time: Time,
}
impl Index {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let mut lock = p.lock().await;
        let tasks = lock.list_tasks()?;
        let files = lock.list_files()?;
        drop(lock);
        Ok(Self {
            tasks,
            files,
            time: Time::default(),
        })
    }
}

#[derive(Default)]
pub enum Time {
    #[default]
    Now,
    Scheduled(DateTime<Utc>),
    Recurring(Vec<NaiveTime>),
}
impl Time {
    pub fn is_now(&self) -> bool {
        match self {
            Time::Now => true,
            Time::Scheduled(_) => false,
            Time::Recurring(_) => false,
        }
    }
    pub fn is_scheduled(&self) -> bool {
        match self {
            Time::Now => false,
            Time::Scheduled(_) => true,
            Time::Recurring(_) => false,
        }
    }
    pub fn is_recurring(&self) -> bool {
        match self {
            Time::Now => false,
            Time::Scheduled(_) => false,
            Time::Recurring(_) => true,
        }
    }
}

#[derive(Template)]
#[template(path = "datepicker.html")]
pub struct DatePicker {
    pub time: Time,
}
impl DatePicker {
    pub async fn get(
        Query(q): Query<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, Response> {
        let time = match q.get("type").map(String::as_str).unwrap_or("now") {
            "now" => Time::Now,
            "scheduled" => Time::Scheduled(Default::default()),
            "recurring" => Time::Recurring(Default::default()),
            _ => return Err((StatusCode::BAD_REQUEST, "Invalid value for `type`").into_response()),
        };
        // TODO: recurring: render multiple datepickers, preferably keep state when changing count?

        Ok(Self { time })
    }
}

#[derive(Template)]
#[template(path = "filepicker.html")]
pub struct FilePicker {
    pub files: Vec<String>,
}

#[derive(Template)]
#[template(path = "form.html")]
pub struct TaskForm {
    pub files: Vec<String>,
    pub time: Time,
}
impl TaskForm {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let files = p.lock().await.list_files()?;
        Ok(Self {
            files,
            time: Time::default(),
        })
    }
}

#[derive(Template)]
#[template(path = "tasks.html")]
pub struct Tasks {
    pub tasks: Vec<Task>,
}
impl Tasks {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let tasks = p.lock().await.list_tasks()?;
        Ok(Self { tasks })
    }
    pub async fn post(State(p): AppState) -> Result<impl IntoResponse, Response> {
        TaskForm::get(State(p)).await
    }
    pub async fn delete(State(p): AppState) -> Result<impl IntoResponse, Response> {
        TaskForm::get(State(p)).await
    }
}

#[derive(Template)]
#[template(path = "files.html")]
pub struct Files {
    pub files: Vec<String>,
}
impl Files {
    pub async fn get(State(p): AppState) -> Result<impl IntoResponse, Response> {
        let files = p.lock().await.list_files()?;
        Ok(Self { files })
    }
    pub async fn post(
        State(p): AppState,
        mut form: Multipart,
    ) -> Result<impl IntoResponse, Response> {
        while let Some(field) = form.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            if name != "file" {
                continue;
            };
            let fname = field.file_name().unwrap().to_string();
            let data = field.bytes().await.unwrap();

            if let Err(e) = db::insert_file(
                &*p.conn.lock().await,
                File {
                    name: fname.clone(),
                    data,
                },
            ) {
                error!("failed to save file: {fname}\n{e:#?}");
                return Err(err_to_reply(
                    e,
                    &fname,
                    "Failed to save file",
                    StatusCode::INSUFFICIENT_STORAGE,
                ));
            }

            return updated_files(p).await;
        }
        Err((StatusCode::BAD_REQUEST, "error").into_response())
    }
    pub async fn delete(
        State(p): AppState,
        Path(fname): Path<String>,
    ) -> Result<impl IntoResponse, Response> {
        if let Err(e) = db::delete_file(&*p.conn.lock().await, &fname) {
            error!("failed to delete file: {fname}\n{e:#?}");
            return Err(err_to_reply(
                e,
                &fname,
                "Failed to delete file",
                StatusCode::NOT_FOUND,
            ));
        }
        updated_files(p).await
    }
}

/// to be sent back when the files were mutated, as these depend on that data
async fn updated_files(p: Arc<Player>) -> Result<impl IntoResponse, Response> {
    let files = p.lock().await.list_files()?;

    let file_list = Files {
        files: files.clone(),
    }
    .render()
    .map_err(|e| err_to_reply(e, "", "Failed to render", StatusCode::INTERNAL_SERVER_ERROR))?;

    let form = FilePicker { files }
        .render()
        .map_err(|e| err_to_reply(e, "", "Failed to render", StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok(file_list + "\n\n" + &form)
}
