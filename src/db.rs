use crate::{
    player::Player,
    scheduler::{schedule_recurring, schedule_task},
    File, Task,
};
use chrono::{NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) type Db = Arc<Mutex<Connection>>;

const DB_FILE: &str = "./csengo.db";

// the format of recurring times in the db
pub(crate) static TIMEFMT: &str = "%H:%M";

pub(crate) fn init() -> Result<(Connection, bool)> {
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
    Ok((conn, db_new))
}
pub(crate) async fn load(player: Arc<Player>) -> anyhow::Result<usize> {
    let conn = &*player.conn.lock().await;
    let tasks = list_tasks(conn)?;
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
                    delete_task(conn, &name)?;
                    len -= 1;
                    continue;
                }
                schedule_task(name, file_name, &time, player.clone())?;
            }
            Task::Recurring {
                name,
                file_name,
                time,
            } => schedule_recurring(name, file_name, time, player.clone())?,
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
pub(crate) fn list_files(conn: &Connection) -> Result<Vec<String>> {
    let mut s = conn.prepare("SELECT name FROM files")?;
    let res = s.query_map([], |r| r.get(0))?;
    res.collect()
}
pub(crate) fn get_file(conn: &Connection, name: &str) -> Result<File> {
    conn.query_row("SELECT * FROM files WHERE name == ?", (name,), |r| {
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
pub(crate) fn list_tasks(conn: &Connection) -> Result<Vec<Task>> {
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
                        NaiveTime::parse_from_str(s, TIMEFMT).map_err(|e| {
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
pub(crate) fn delete_task(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM tasks WHERE name == ?", (name,))? == 1)
}
pub(crate) fn exists_task(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT name FROM tasks WHERE name == ?",
            (name,),
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
