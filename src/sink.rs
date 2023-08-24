// low-level audio sink implementation
// for a higher-level interface, see `src/player.rs`
use crate::player::NowPlaying;
use rodio::{
    source::{Empty, Zero},
    OutputStream, OutputStreamHandle, Source,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{
    oneshot,
    watch::{self, Receiver, Sender},
};

struct Output {
    controller: Arc<Controller>,
    np: Track,
    np_tx: Sender<Option<NowPlaying>>,
}
impl Source for Output {
    // should never return `None` or `0`
    // see [`rodio::queue::SourcesQueueOutput::current_frame_len`]
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        if let Some(len) = self.np.src.current_frame_len() {
            if len > 0 {
                return Some(len);
            }
        }

        let (lower, _) = self.np.src.size_hint();
        if lower > 0 {
            return Some(lower);
        }

        Some(512)
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.np.src.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.np.src.sample_rate()
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
            if let Some(sample) = self.np.src.next() {
                return Some(sample);
            }

            // signal end of track
            if let Some(tx) = self.np.signal.take() {
                tx.send(()).unwrap();
            }
            if let Some(name) = &self.np.name {
                debug!("end of track: {:?}", name);
            }

            // get next track
            let mut q = self.controller.q.lock().unwrap();
            if !q.is_empty() {
                self.np = q.pop_front().unwrap();
                if let Some(name) = &self.np.name {
                    info!("playing: {:?}", name);
                }
            } else {
                // play a bit of silence
                self.np = Track {
                    src: Box::new(Zero::new(1, 44100).take_duration(Duration::from_millis(500))), // this will give every play a worst-case 500ms delay, but in the context of this program and the benefits of lower resource usage, that's acceptable
                    name: None,
                    signal: None,
                };
            }

            // signal start of track
            self.np_tx.send_if_modified(|np| {
                if self.np.name.is_none() && np.is_none() {
                    return false;
                }
                *np = self.np.name.clone().map(|name| NowPlaying { name });
                true
            });
        }
    }
}

pub struct Controller {
    pub q: Mutex<VecDeque<Track>>,
    controls: Arc<Controls>,
}
impl Controller {
    pub fn init() -> (
        Arc<Controller>,
        Receiver<Option<NowPlaying>>,
        OutputStream,
        OutputStreamHandle,
    ) {
        let (_stream, _handle) = OutputStream::try_default().expect("failed to find output device");
        let (np_tx, np_rx) = watch::channel(None);
        let controller = Arc::new(Controller {
            q: Mutex::new(VecDeque::new()),
            controls: Arc::new(Controls {
                stop: AtomicBool::new(false),
            }),
        });
        let output = Output {
            controller: Arc::clone(&controller),
            np: Track {
                src: Box::new(Empty::<f32>::new()),
                name: None,
                signal: None,
            },
            np_tx,
        };

        // exit the tokio context to be able to use blocking functions
        let h = _handle.clone();
        tokio::task::spawn_blocking(move || h.play_raw(output).unwrap());

        (controller, np_rx, _stream, _handle)
    }

    pub fn append(&self, mut t: Track) {
        // resume playback
        self.controls.stop.store(false, Ordering::Relaxed);

        let c = Arc::clone(&self.controls);
        t.src = Box::new(t.src.stoppable().periodic_access(
            Duration::from_millis(10),
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
    pub signal: Option<oneshot::Sender<()>>,
    pub src: Box<dyn Source<Item = f32> + Send + Sync>,
}
