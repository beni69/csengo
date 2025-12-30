use crate::{metrics as m, player::Player, scheduler::schedule, File, Task};
use chrono::{Local, NaiveTime};
use rusqlite::{params, Connection, Error, Result, Row};
use std::{path::Path, sync::Arc, time::Instant};
use tokio::sync::Mutex;

pub type Db = Arc<Mutex<Connection>>;

const DB_FILE: &str = "./csengo.db";

/// to be incremented on schema changes
pub const DB_VERSION: u32 = 1;

/// this initializes the db to the latest schema version
const CREATE_TABLES: &str = "
CREATE TABLE tasks (
    type      TEXT NOT NULL,
    name      TEXT PRIMARY KEY,
    priority  INTEGER NOT NULL,
    file_name TEXT NOT NULL,
    time      TEXT
), STRICT;
CREATE TABLE files (
    name      TEXT PRIMARY KEY,
    data      BLOB
), STRICT;
";

// the format of recurring times in the db
pub static TIMEFMT: &str = "%H:%M";

pub fn init() -> Result<(Db, bool)> {
    let db_new = !Path::new(DB_FILE).try_exists().unwrap_or(false);
    let mut conn = Connection::open(DB_FILE)?;
    if db_new {
        info!("no db found, initializing...");
        let tr = conn.transaction()?;
        tr.execute_batch(CREATE_TABLES)?;
        tr.pragma_update(None, "user_version", DB_VERSION)?;
        tr.commit()?;
    } else {
        let version: u32 =
            conn.pragma_query_value(None, "user_version", |r| r.get("user_version"))?;

        if version != DB_VERSION {
            migrate(&conn, version)?;
        }
    }
    info!("db connect successful");
    Ok((Arc::new(Mutex::new(conn)), db_new))
}

fn migrate(conn: &Connection, version: u32) -> Result<()> {
    info!("existing db at v{version} (latest: v{DB_VERSION}), running migrations");

    for i in version..DB_VERSION {
        match i {
            0 => {
                conn.execute_batch(&format!(
                    "BEGIN EXCLUSIVE;
                     ALTER TABLE tasks RENAME TO old_tasks;
                     ALTER TABLE files RENAME TO old_files;
                     {CREATE_TABLES}
                     INSERT INTO tasks SELECT *, 0 as priority FROM old_tasks;
                     INSERT INTO files SELECT * FROM old_files;
                     COMMIT;"
                ))?;
            }
            DB_VERSION.. => (),
        }
        debug!(
            "applied {} migrations (v{} -> v{DB_VERSION})",
            DB_VERSION - version - i,
            i
        );
    }
    conn.pragma_update(None, "user_version", DB_VERSION)?;
    Ok(())
}

pub async fn load(player: Player) -> anyhow::Result<usize> {
    let conn = &*player.conn.lock().await;
    let tasks = list_tasks(conn)?;
    let mut len = tasks.len();
    for task in tasks {
        let name = task.get_name().to_owned();

        if let Task::Scheduled { time, .. } = &task {
            if *time < Local::now() {
                warn!("{name}: Scheduled task missed, deleting it");
                delete_task(conn, &name)?;
                continue;
            }
        }

        if let Err(e) = schedule(task, player.clone()).await {
            len -= 1;
            warn!("{name} failed to schedule: {e}");
        }
    }
    Ok(len)
}

pub fn insert_file(conn: &Connection, file: File) -> Result<()> {
    conn.execute(
        "INSERT INTO files (name, data) VALUES (?1, ?2)",
        params![file.name, file.data.to_vec()],
    )
    .map(|_| ())
}
pub fn list_files(conn: &Connection) -> Result<Vec<String>> {
    let mut s = conn.prepare("SELECT name FROM files")?;
    let res = s.query_map([], |r| r.get(0))?;
    res.collect()
}
pub fn get_file(conn: &Connection, name: &str) -> Result<File> {
    conn.query_row("SELECT * FROM files WHERE name == ?", (name,), |r| {
        Ok(File {
            name: r.get(0)?,
            data: r.get::<_, Vec<u8>>(1)?.into(),
        })
    })
}
pub fn delete_file(conn: &Connection, name: &str) -> Result<()> {
    conn.execute("DELETE FROM files WHERE name == ?", (name,))
        .map(|_| ())
}

pub fn insert_task(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks (type, name, priority, file_name, time) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task.get_type(),
            task.get_name(),
            task.get_priority(),
            task.get_file_name(),
            task.time_to_str()
        ],
    )
    .map(|_| ())
}
pub fn list_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut s = conn.prepare("SELECT * FROM tasks")?;
    let res = s.query_map([], parse_task)?;
    res.collect()
}
pub fn get_task(conn: &Connection, name: &str) -> Result<Task> {
    conn.query_row("SELECT * FROM tasks WHERE name == ?", (name,), parse_task)
}
pub fn delete_task(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM tasks WHERE name == ?", (name,))? == 1)
}

fn parse_task(r: &Row) -> Result<Task, Error> {
    Ok(match r.get::<_, String>(0)?.as_str() {
        "now" => Task::Now {
            name: r.get(1)?,
            priority: r.get(2)?,
            file_name: r.get(3)?,
        },
        "scheduled" => Task::Scheduled {
            name: r.get(1)?,
            priority: r.get(2)?,
            file_name: r.get(3)?,
            time: r.get(4)?,
        },
        "recurring" => Task::Recurring {
            name: r.get(1)?,
            priority: r.get(2)?,
            file_name: r.get(3)?,
            time: r
                .get::<_, String>(4)?
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
}

pub fn db_err(e: rusqlite::Error) -> anyhow::Error {
    match e {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                ..
            },
            _,
        ) => anyhow::anyhow!("Name already in use"),
        _ => anyhow::anyhow!("Unknown database error: e"),
    }
}

/// query file statistics (count and total size) and update metrics
pub fn update_file_stats(conn: &Connection) {
    let start = Instant::now();
    let result: Result<(i64, i64)> = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(LENGTH(data)), 0) FROM files",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    m::record_db_operation("stats", "files", start.elapsed().as_secs_f64());

    match result {
        Ok((count, bytes)) => m::set_file_stats(count, bytes),
        Err(e) => warn!("Failed to query file stats: {e}"),
    }
}
