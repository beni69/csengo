use crate::{
    db,
    server::err_to_reply,
    sink::{Controller, Track},
    Task,
};
use anyhow::Result;
use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use rodio::{
    source::{ChannelVolume, SineWave, UniformSourceIterator},
    Decoder, Source,
};
use rusqlite::Connection;
use serde::Serialize;
use std::{collections::HashMap, io::Cursor, sync::Arc, time::Duration};
use tokio::sync::{
    oneshot,
    watch::{Receiver, Ref},
    Mutex, MutexGuard,
};

#[derive(Clone)]
pub struct Player {
    pub controller: Controller,
    pub conn: db::Db,
    np_rx: Receiver<Option<NowPlaying>>,
    cancel_map: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
}
impl Player {
    pub fn new(
        controller: Controller,
        np_rx: Receiver<Option<NowPlaying>>,
        conn: Connection,
    ) -> Self {
        Player {
            controller,
            conn: Arc::new(Mutex::new(conn)),
            np_rx,
            cancel_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn stop(&self) {
        self.controller.stop();
    }

    pub async fn play_file(&self, fname: &str) -> Result<()> {
        let right = false;
        let file = db::get_file(&*self.conn.lock().await, fname)?;
        self.play_buf(file.data, fname, right)
    }
    pub fn play_buf(&self, buf: Bytes, fname: &str, right: bool) -> Result<()> {
        let src = Decoder::new(Cursor::new(buf))?;
        // https://github.com/RustAudio/rodio/pull/493: ChannelVolume distorts the audio if the
        // input sample rate isn't constant, so we manually normalize it here.
        let src: UniformSourceIterator<_, f32> = UniformSourceIterator::new(src, 2, 48000);
        let src = ChannelVolume::new(src, vec![0.5, if right { 0.5 } else { 0.0 }]);

        self.controller.append(Track {
            src: Box::new(src),
            name: Some(fname.into()),
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
    pub fn np_stream(&self) -> impl futures_util::Stream<Item = Option<NowPlaying>> {
        fn inner(r: tokio::sync::watch::Ref<'_, Option<NowPlaying>>) -> Option<NowPlaying> {
            r.to_owned()
        }
        let mut rx = self.np_realtime();
        async_stream::stream! {
            let data = inner(rx.borrow());
            yield data;
            while rx.changed().await.is_ok() {
                let data = inner(rx.borrow());
                yield data;
            }
        }
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
        });
    }

    pub async fn db_name<T: Serialize>(
        &self,
        f: impl Fn(&Connection, &str) -> rusqlite::Result<T>,
        name: &str,
    ) -> rusqlite::Result<T> {
        f(&*self.conn.lock().await, name)
    }

    pub async fn create_cancel(&self, key: String) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.cancel_map.lock().await.insert(key, tx);
        rx
    }
    pub async fn delete_cancel(&self, key: &str) -> Option<oneshot::Sender<()>> {
        self.cancel_map.lock().await.remove(key)
    }
    pub async fn cancel(&self, key: &str) -> Result<()> {
        match self.delete_cancel(key).await {
            Some(tx) => tx.send(()).map_err(|_| anyhow::anyhow!("Failed to cancel")),
            None => Ok(()),
        }
    }

    pub async fn lock(&self) -> PlayerLock {
        let lock = self.conn.lock().await;
        PlayerLock { lock }
    }
}

pub struct PlayerLock<'a> {
    pub lock: MutexGuard<'a, Connection>,
}
impl PlayerLock<'_> {
    pub fn list_tasks(&mut self) -> Result<Vec<Task>, Response> {
        db::list_tasks(&self.lock).map_err(|e| {
            err_to_reply(
                e.into(),
                "List tasks",
                "Failed to get tasks",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })
    }
    pub fn list_files(&mut self) -> Result<Vec<String>, Response> {
        db::list_files(&self.lock).map_err(|e| {
            err_to_reply(
                e.into(),
                "List files",
                "Failed to get files",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NowPlaying {
    pub name: String,
    // pos: Duration,
    // len: Duration,
}
