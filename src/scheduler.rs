use crate::{db, mail, player::Player, Task};
use chrono::Utc;
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::sync::Arc;
use tokio::{
    select,
    time::{interval_at, Duration, Instant, MissedTickBehavior},
};

// !TODO: interpret db timestamps as local time, but api requests as utc and convert before storing in db

pub fn schedule(task: Task, player: Arc<Player>) -> anyhow::Result<()> {
    match task {
        Task::Scheduled {
            name,
            file_name,
            time,
        } => {
            // return if the date was in the past
            // there has to be a better way to do this
            (time - Utc::now()).to_std()?;

            tokio::task::spawn(async move {
                let diff = (time - Utc::now()).to_std().unwrap();
                debug!("{}: waiting {}s", name, diff.as_secs());

                let rx = player.create_cancel(name.to_owned()).await;
                if select! {
                    biased;
                    _ = rx => true,
                    _ = tokio::time::sleep(diff) => false,
                } {
                    info!("{name}: cancelled");
                    return;
                }

                if let Err(e) = player.play_file(&file_name).await {
                    error!("error while playing {}:\n{e:#?}", file_name);
                } else {
                    // successful play
                    mail::task_done(&file_name, &time).await;
                }

                if let Err(e) = player.db_name(db::delete_task, &name).await {
                    error!("{name}: failed to delete task after scheduled play\n{e:#?}");
                };
                if player.delete_cancel(&name).await.is_none() {
                    error!("{name}: failed to delete cancel channel");
                };
            });
        }

        Task::Recurring {
            name,
            file_name,
            time: times,
        } => {
            let mut intervals = Vec::with_capacity(times.len());
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
                let mut interval = interval_at(start, Duration::from_secs(24 * 60 * 60));
                interval.set_missed_tick_behavior(MissedTickBehavior::Burst);
                intervals.push(interval);
            }

            tokio::task::spawn(async move {
                let mut rx = player.create_cancel(name.to_owned()).await;

                loop {
                    let mut futures = FuturesUnordered::new();
                    for f in intervals.iter_mut() {
                        futures.push(f.tick());
                    }

                    if select! {
                        biased;
                        _ = &mut rx => true,
                        _ = futures.next() => false,
                    } {
                        info!("{name}: cancelled");
                        return;
                    }

                    if let Err(e) = player.play_file(&file_name).await {
                        error!("{name}: recurring play failed\n{e:#?}");
                    } else {
                        debug!("{name}: added to queue, going back to sleep");
                    }
                }
            });
        }

        _ => unreachable!("impossible to schedule a `Now`"),
    }
    Ok(())
}
