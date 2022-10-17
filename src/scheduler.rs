use crate::{db, play_file};
use chrono::{DateTime, NaiveTime, Utc};
use rodio::Sink;
use std::sync::Arc;
use tokio::time::{interval_at, Duration, Instant, MissedTickBehavior};

pub(crate) fn schedule_task(
    name: String,
    file_name: String,
    time: &DateTime<Utc>,
    conn: db::Db,
    sink: Arc<Sink>,
) -> anyhow::Result<()> {
    let diff = (*time - Utc::now()).to_std()?;
    tokio::task::spawn(async move {
        debug!("{}: waiting {}s", name, diff.as_secs());
        tokio::time::sleep(diff).await;
        {
            match db::exists_task(&*conn.lock().await, &name) {
                Ok(b) => {
                    if !b {
                        warn!("{name}: would've played, but the task was since deleted");
                        return;
                    }
                }
                Err(e) => {
                    error!("{name}: failed to check task\n{e:#?}");
                }
            }
        }
        if let Err(e) = play_file(&file_name, &conn, &sink).await {
            error!("error while playing {}:\n{e:#?}", &file_name);
        }
        if let Err(e) = db::delete_task(&*conn.lock().await, &name) {
            error!("{name}: failed to delete task after scheduled play\n{e:#?}");
        };
    });

    Ok(())
}
pub(crate) fn schedule_recurring(
    name: String,
    file_name: String,
    times: Vec<NaiveTime>,
    conn: db::Db,
    sink: Arc<Sink>,
) -> anyhow::Result<()> {
    for time in times {
        let diff: Duration = match (time - Utc::now().time()).to_std() {
            Ok(d) => d,
            Err(_) => ((Utc::today()
                .and_time(time)
                .expect("datetime construction failed")
                + chrono::Duration::days(1))
                - (Utc::now()))
            .to_std()
            .expect("time went backwards"),
        };
        debug!("{name} (recurring): waiting {diff:.0?} for first play");
        let start: Instant = Instant::now() + diff;
        let name = name.clone();
        let fname = file_name.clone();
        let conn = conn.clone();
        let sink = sink.clone();
        tokio::task::spawn(async move {
            let mut interval = interval_at(start, Duration::from_secs(24 * 60 * 60));
            interval.set_missed_tick_behavior(MissedTickBehavior::Burst);

            loop {
                interval.tick().await;
                if let Err(e) = play_file(&fname, &conn, &sink).await {
                    error!("{name}: recurring play failed\n{e:#?}");
                } else {
                    debug!("{name}: added to queue, going back to sleep");
                }
            }
        });
    }

    Ok(())
}
