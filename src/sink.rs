// low-level audio sink implementation
// for a higher-level interface, see `src/player.rs`
use crate::player::NowPlaying;
use rodio::{
    source::{Empty, Zero},
    OutputStream, Source,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch::{self, Receiver, Sender};

struct Output {
    controller: Controller,
    track: Track,
    np_tx: Sender<Option<NowPlaying>>,
}
impl Source for Output {
    // should never return `None` or `0`
    // see [`rodio::queue::SourcesQueueOutput::current_frame_len`]
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        if let Some(len) = self.track.src.current_frame_len() {
            if len > 0 {
                return Some(len);
            }
        }

        let (lower, _) = self.track.src.size_hint();
        if lower > 0 {
            return Some(lower);
        }

        Some(512)
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.track.src.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.track.src.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
impl Iterator for Output {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // keep playing current track
            if let Some(sample) = self.track.src.next() {
                return Some(sample);
            }

            if let Some(name) = &self.track.name {
                debug!("end of track: {:?}", name);
            }

            // get next track
            let mut q = self.controller.q.lock().unwrap();
            if let Some(next) = q.pop_front() {
                self.track = next;
                if let Some(name) = &self.track.name {
                    info!("playing: {:?}", name);
                }
            } else {
                // play a bit of silence
                self.track = Track {
                    // this will give every play a worst-case 500ms delay, but in the context of this program and the benefits of lower resource usage, that's acceptable
                    src: Box::new(Zero::new(1, 44100).take_duration(Duration::from_millis(500))),
                    name: None,
                };
            }

            // signal start of track
            self.np_tx
                .send_if_modified(|prev| match (&prev, self.track.name.to_owned()) {
                    (None, None) => false,
                    (Some(_), None) => {
                        *prev = None;
                        true
                    }
                    (_, Some(name)) => {
                        *prev = Some(NowPlaying {
                            name,
                            len: self.track.src.total_duration(),
                            started: Instant::now(),
                        });
                        true
                    }
                });
        }
    }
}

#[derive(Clone)]
pub struct Controller {
    pub q: Arc<Mutex<VecDeque<Track>>>,
    controls: Arc<Controls>,
}
impl Controller {
    pub fn init() -> (Controller, Receiver<Option<NowPlaying>>) {
        let (_stream, _handle) = OutputStream::try_default().expect("failed to find output device");
        let (np_tx, np_rx) = watch::channel(None);
        let controller = Controller {
            q: Arc::new(Mutex::new(VecDeque::new())),
            controls: Arc::new(Controls {
                stop: AtomicBool::new(false),
            }),
        };
        let output = Output {
            controller: controller.clone(),
            track: Track {
                src: Box::new(Empty::new()),
                name: None,
            },
            np_tx,
        };

        // exit the tokio async thread to be able to use blocking functions
        tokio::task::spawn_blocking(move || {
            _handle.play_raw(output).unwrap();
            Box::leak(Box::new(_handle));
        });

        Box::leak(Box::new(_stream));

        (controller, np_rx)
    }

    pub fn append(&self, mut t: Track) {
        // resume playback
        self.controls.stop.store(false, Ordering::Relaxed);

        let c = self.controls.clone();
        t.src = Box::new(t.src.stoppable().periodic_access(
            Duration::from_millis(420),
            move |src| {
                if c.stop.load(Ordering::Relaxed) {
                    src.stop();
                }
            },
        ));

        let mut q = self.q.lock().unwrap();
        q.push_back(t);
    }

    pub fn stop(&self) {
        self.q.lock().unwrap().clear();
        self.controls.stop.store(true, Ordering::Relaxed);
    }
}

struct Controls {
    stop: AtomicBool, // atomic for interior mutability
}

pub struct Track {
    pub name: Option<String>,
    pub src: Box<dyn Source<Item = f32> + Send + Sync>,
}
