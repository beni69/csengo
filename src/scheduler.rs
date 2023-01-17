use crate::{db, mail, player::Player};
use chrono::{DateTime, NaiveTime, Utc};
use std::sync::Arc;
use tokio::time::{interval_at, Duration, Instant, MissedTickBehavior};

// !TODO: interpret db timestamps as local time, but api requests as utc and convert before storing in db

pub(crate) fn schedule_task(
    name: String,
    file_name: String,
    time: DateTime<Utc>,
    player: Arc<Player>,
) -> anyhow::Result<()> {
    let diff = (time - Utc::now()).to_std()?;
    tokio::task::spawn(async move {
        debug!("{}: waiting {}s", name, diff.as_secs());
        tokio::time::sleep(diff).await;
        {
            match player.db_name(db::exists_task, &name).await {
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
        if let Err(e) = player.play_file(&file_name).await {
            error!("error while playing {}:\n{e:#?}", &file_name);
        } else {
            // successful play
            tokio::task::spawn(async move {
                mail::task_done(&file_name, &time).await;
            });
        }
        if let Err(e) = player.db_name(db::delete_task, &name).await {
            error!("{name}: failed to delete task after scheduled play\n{e:#?}");
        };
    });

    Ok(())
}
pub(crate) fn schedule_recurring(
    name: String,
    file_name: String,
    times: Vec<NaiveTime>,
    player: Arc<Player>,
) -> anyhow::Result<()> {
    for time in times {
        let (diff, tmrw): (Duration, bool) = match (time - Utc::now().time()).to_std() {
            Ok(d) => (d, false),
            Err(_) => (
                ((Utc::now() + chrono::Duration::days(1))
                    .date_naive()
                    .and_time(time)
                    - Utc::now().naive_utc())
                .to_std()?,
                true,
            ),
        };
        debug!(
            "{name} (recurring): waiting {diff:.0?} for first play {}",
            if tmrw { "+" } else { "" }
        );
        let start: Instant = Instant::now() + diff;
        let name = name.clone();
        let fname = file_name.clone();
        let player = player.clone();
        tokio::task::spawn(async move {
            let mut interval = interval_at(start, Duration::from_secs(24 * 60 * 60));
            interval.set_missed_tick_behavior(MissedTickBehavior::Burst);

            loop {
                interval.tick().await;
                if let Err(e) = player.play_file(&fname).await {
                    error!("{name}: recurring play failed\n{e:#?}");
                } else {
                    debug!("{name}: added to queue, going back to sleep");
                }
            }
        });
    }

    Ok(())
}
