use crate::{db, mail, metrics as m, player::Player, templates::dur_human, Task};
use chrono::{DateTime, Local, NaiveTime, TimeZone};
use tokio::{select, time::Duration};

// get next time to play for recurring tasks
// handles daylight savings time by subtracting timezone-aware chrono DateTimes
fn duration_until_next(times: &[NaiveTime]) -> (chrono::Duration, DateTime<Local>) {
    let now = Local::now();

    times
        .iter()
        .filter_map(|&target| {
            let today = now.date_naive().and_time(target);

            if let Some(dt) = Local.from_local_datetime(&today).single() {
                if dt > now {
                    return Some((dt - now, dt));
                }
            }

            // fall back to tomorrow
            let tomorrow = (now + chrono::Duration::days(1))
                .date_naive()
                .and_time(target);

            Local
                .from_local_datetime(&tomorrow)
                .single()
                .map(|dt| (dt - now, dt))
        })
        .min_by_key(|(duration, _)| *duration)
        .expect("at least one valid time should exist")
}

pub async fn schedule(task: Task, player: Player) -> anyhow::Result<()> {
    match task {
        Task::Now {
            name,
            file_name,
            priority,
        } => {
            if let Err(e) = player.play_file(&file_name, priority).await {
                m::record_playback_failure("now", &name);
                return Err(e);
            }
            m::record_playback_success("now", &name);
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

            m::inc_active_tasks("scheduled");

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
                    m::dec_active_tasks("scheduled");
                    return;
                }

                // record drift
                let now = Local::now();
                let drift = (now - time).num_milliseconds().abs() as f64 / 1000.0;
                m::record_drift("scheduled", &name, drift);

                if let Err(e) = player.play_file(&file_name, priority).await {
                    error!("error while playing {}:\n{e:#?}", file_name);
                    m::record_playback_failure("scheduled", &name);
                } else {
                    // successful play
                    m::record_playback_success("scheduled", &name);
                    mail::task_done(&file_name, &time).await;
                }

                m::dec_active_tasks("scheduled");

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
            weekday_filter,
        } => {
            m::inc_active_tasks("recurring");

            tokio::task::spawn(async move {
                let mut rx = player.create_cancel(name.to_owned()).await;

                loop {
                    let (diff, expected_time) = duration_until_next(&times);
                    let is_tomorrow = expected_time.date_naive() != Local::now().date_naive();
                    debug!(
                        "{name} (recurring): {} ({}{})",
                        dur_human(&diff).0,
                        expected_time.time().format("%H:%M"),
                        if is_tomorrow { "+" } else { "" }
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
                        m::dec_active_tasks("recurring");
                        return;
                    }

                    // record drift
                    let now = Local::now();
                    let drift = (now - expected_time).num_milliseconds().abs() as f64 / 1000.0;
                    m::record_drift("recurring", &name, drift);

                    if let Err(e) = player.play_file(&file_name, priority).await {
                        error!("{name}: recurring play failed\n{e:#?}");
                        m::record_playback_failure("recurring", &name);
                    } else {
                        debug!("{name}: added to queue, going back to sleep");
                        m::record_playback_success("recurring", &name);
                    }
                }
            });
        }
    }
    Ok(())
}
