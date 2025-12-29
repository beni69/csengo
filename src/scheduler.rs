use crate::{db, mail, player::Player, templates::dur_human, Task};
use chrono::{Local, NaiveTime, TimeZone};
use tokio::{select, time::Duration};

/// Calculate the duration until the next occurrence of any of the given target times.
/// Uses wall-clock time via chrono, so DST transitions are handled correctly.
/// If a target time falls into a DST gap (doesn't exist), it's skipped for that day.
fn duration_until_next(times: &[NaiveTime]) -> (NaiveTime, chrono::Duration) {
    let now = Local::now();

    times
        .iter()
        .filter_map(|&target| {
            let today = now.date_naive().and_time(target);

            // Try today first
            if let Some(dt) = Local.from_local_datetime(&today).single() {
                if dt > now {
                    return Some((target, dt - now));
                }
            }

            // Fall back to tomorrow
            let tomorrow = (now + chrono::Duration::days(1))
                .date_naive()
                .and_time(target);

            Local
                .from_local_datetime(&tomorrow)
                .single()
                .map(|dt| (target, dt - now))
        })
        .min_by_key(|(_, duration)| *duration)
        .expect("at least one valid time should exist")
}

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
            tokio::task::spawn(async move {
                let mut rx = player.create_cancel(name.to_owned()).await;

                loop {
                    let (next_time, diff) = duration_until_next(&times);
                    debug!(
                        "{name} (recurring): {} until {next_time}",
                        dur_human(&diff).0
                    );

                    let sleep_duration = match diff.to_std() {
                        Ok(d) => d,
                        Err(_) => {
                            warn!("{name}: unexpected negative duration, executing immediately");
                            Duration::ZERO
                        }
                    };

                    if select! {
                        biased;
                        _ = &mut rx => true,
                        _ = tokio::time::sleep(sleep_duration) => false,
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
