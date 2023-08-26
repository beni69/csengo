use crate::{db, mail, player::Player, templates::dur_human, Task};
use chrono::Local;
use futures_util::{stream::FuturesUnordered, StreamExt};
use tokio::{
    select,
    time::{interval_at, Duration, Instant, MissedTickBehavior},
};

pub async fn schedule(task: Task, player: Player) -> anyhow::Result<()> {
    match task {
        Task::Now {
            file_name,
            priority,
            ..
        } => {
            player.play_file(&file_name, priority).await?;
        }

        Task::Scheduled {
            name,
            priority,
            file_name,
            time,
        } => {
            // return if the date was in the past
            // there has to be a better way to do this
            (time - Local::now()).to_std()?;

            tokio::task::spawn(async move {
                let diff = time - Local::now();
                debug!("{}: {}", name, dur_human(&diff).0);
                let diff = diff.to_std().unwrap();

                let rx = player.create_cancel(name.to_owned()).await;
                if select! {
                    biased;
                    _ = rx => true,
                    _ = tokio::time::sleep(diff) => false,
                } {
                    info!("{name}: cancelled");
                    return;
                }

                if let Err(e) = player.play_file(&file_name, priority).await {
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
            priority,
            file_name,
            time: times,
        } => {
            let mut intervals = Vec::with_capacity(times.len());
            for time in times {
                let (diff, tmrw): (Duration, bool) = match (time - Local::now().time()).to_std() {
                    Ok(d) => (d, false),
                    Err(_) => (
                        ((Local::now() + chrono::Duration::days(1))
                            .date_naive()
                            .and_time(time)
                            - Local::now().naive_utc())
                        .to_std()?,
                        true,
                    ),
                };
                debug!(
                    "{name} (recurring): {} {}",
                    dur_human(&chrono::Duration::from_std(diff).unwrap()).0,
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

                    if let Err(e) = player.play_file(&file_name, priority).await {
                        error!("{name}: recurring play failed\n{e:#?}");
                    } else {
                        debug!("{name}: added to queue, going back to sleep");
                    }
                }
            });
        }
    }
    Ok(())
}
