use crate::{schedule_recurring, schedule_task, File, Task};
use chrono::{NaiveTime, Utc};
use rodio::Sink;
use rusqlite::{params, Connection, Result};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) type Db = Arc<Mutex<Connection>>;

const DB_FILE: &str = "./csengo.db";

pub(crate) fn init() -> Result<(Db, bool)> {
    let db_new = !Path::new(DB_FILE).try_exists().unwrap_or(false);
    let conn = Connection::open(DB_FILE)?;
    if db_new {
        info!("initializing db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
            type      TEXT NOT NULL,
            name      TEXT PRIMARY KEY,
            file_name TEXT NOT NULL,
            time      TEXT
        );
        CREATE TABLE IF NOT EXISTS files (
            name      TEXT PRIMARY KEY,
            data      BLOB
        );",
        )?;
    }
    info!("db init successful");
    Ok((Arc::new(Mutex::new(conn)), db_new))
}
pub(crate) async fn load(conn: Db, sink: Arc<Sink>) -> anyhow::Result<usize> {
    let tasks = list_tasks(conn.clone()).await?;
    let mut len = tasks.len();
    for task in tasks {
        match task {
            Task::Scheduled {
                name,
                file_name,
                time,
            } => {
                if (time - Utc::now()) < chrono::Duration::zero() {
                    debug!("{name}: expired, skipping");
                    delete_task(&*conn.lock().await, &name)?;
                    len -= 1;
                    continue;
                }
                schedule_task(name, file_name, &time, conn.clone(), sink.clone())?;
            }
            Task::Recurring {
                name,
                file_name,
                time,
            } => schedule_recurring(name, file_name, time, conn.clone(), sink.clone())?,
            _ => unreachable!("Task::Now shouldn't be stored in the db"),
        }
    }
    Ok(len)
}

pub(crate) fn insert_file(conn: &Connection, file: File) -> Result<()> {
    conn.execute(
        "INSERT INTO files (name, data) VALUES (?1, ?2)",
        params![file.name, file.data.to_vec()],
    )
    .map(|_| ())
}
pub(crate) async fn list_files(conn: Db) -> Result<Vec<String>> {
    let conn = conn.lock().await;
    let mut s = conn.prepare("SELECT name FROM files")?;
    let res = s.query_map([], |r| r.get(0))?;
    res.collect()
}
pub(crate) fn get_file(conn: &Connection, name: &str) -> Result<File> {
    conn.query_row("SELECT * FROM files WHERE name == ?", [name], |r| {
        Ok(File {
            name: r.get(0)?,
            data: r.get::<_, Vec<u8>>(1)?.into(),
        })
    })
}

pub(crate) fn insert_task(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks (type, name, file_name, time) VALUES (?1, ?2, ?3, ?4)",
        params![
            task.get_type(),
            task.get_name(),
            task.get_file_name(),
            task.time_to_str()
        ],
    )
    .map(|_| ())
}
pub(crate) async fn list_tasks(conn: Db) -> Result<Vec<Task>> {
    let conn = conn.lock().await;
    let mut s = conn.prepare("SELECT * FROM tasks")?;
    let res = s.query_map([], |r| {
        Ok(match r.get::<_, String>(0)?.as_str() {
            "now" => Task::Now {
                name: r.get(1)?,
                file_name: r.get(2)?,
            },
            "scheduled" => Task::Scheduled {
                name: r.get(1)?,
                file_name: r.get(2)?,
                time: r.get(3)?,
            },
            "recurring" => Task::Recurring {
                name: r.get(1)?,
                file_name: r.get(2)?,
                // time: r     .get::<_, String>(3)?     .split(';')     .map(|s| {         DateTime::parse_from_rfc3339(s)             .map(|v| v.into())             .map_err(|e| {                 rusqlite::Error::FromSqlConversionFailure(                     3,                     rusqlite::types::Type::Text,                     Box::new(e),                 )             })     })     .collect::<Result<Vec<DateTime<Utc>>>>()?,
                time: r
                    .get::<_, String>(3)?
                    .split(';')
                    .map(|s| {
                        s.parse().map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })
                    })
                    .collect::<Result<Vec<NaiveTime>>>()?,
            },
            _ => unreachable!(),
        })
    })?;
    res.collect()
}
pub(crate) fn delete_task(conn: &Connection, name: &str) -> Result<()> {
    conn.execute("DELETE FROM tasks WHERE name == ?", [name])
        .map(|_| ())
}
