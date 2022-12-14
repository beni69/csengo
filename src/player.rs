use crate::{
    db,
    sink::{Controller, Track},
};
use anyhow::Result;
use bytes::Bytes;
use rodio::{source::SineWave, Decoder, Source};
use rusqlite::Connection;
use serde::Serialize;
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio::sync::watch::{Receiver, Ref};

pub struct Player {
    pub controller: Arc<Controller>,
    pub conn: db::Db,
    np_rx: Receiver<Option<NowPlaying>>,
}
impl Player {
    pub fn new(
        controller: Arc<Controller>,
        np_rx: Receiver<Option<NowPlaying>>,
        conn: Connection,
    ) -> Arc<Self> {
        Arc::new(Player {
            controller,
            conn: Arc::new(tokio::sync::Mutex::new(conn)),
            np_rx,
        })
    }

    pub fn stop(&self) {
        self.controller.stop();
    }

    pub async fn play_file(&self, fname: &str) -> Result<()> {
        let file = db::get_file(&*self.conn.lock().await, fname)?;
        self.play_buf(file.data, fname)
    }
    pub fn play_buf(&self, buf: Bytes, fname: &str) -> Result<()> {
        let src = Decoder::new(Cursor::new(buf))?;
        self.controller.append(Track {
            src: Box::new(src.convert_samples()),
            name: Some(fname.into()),
            signal: None,
        });
        Ok(())
    }

    pub fn now_playing(&self) -> Ref<Option<NowPlaying>> {
        self.np_rx.borrow()
    }
    pub fn np_realtime(&self) -> Receiver<Option<NowPlaying>> {
        let mut rx = self.np_rx.clone();
        rx.borrow_and_update(); // to make sure to wait for the next value
        rx
    }

    pub fn playtest(&self) {
        // taken from https://docs.rs/rodio
        // Add a dummy source for the sake of the example.
        let source = SineWave::new(880.0)
            .take_duration(Duration::from_secs_f32(1.0))
            .amplify(0.20);
        self.controller.append(Track {
            src: Box::new(source),
            name: Some("playtest".into()),
            signal: None,
        });
    }

    pub async fn db_name<T: Serialize>(
        &self,
        f: impl Fn(&Connection, &str) -> rusqlite::Result<T>,
        name: &str,
    ) -> rusqlite::Result<T> {
        f(&*self.conn.lock().await, name)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NowPlaying {
    pub name: String,
    // pos: Duration,
    // len: Duration,
}
