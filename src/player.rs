use crate::db;
use anyhow::Result;
use bytes::Bytes;
use rodio::{source::SineWave, Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use rusqlite::Connection;
use serde::Serialize;
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub(crate) struct Player {
    pub sink: Sink,
    stream: &'static OutputStreamHandle,
    pub conn: db::Db,
}
impl Player {
    pub fn new(stream: &'static OutputStreamHandle, conn: Connection) -> Arc<Self> {
        let sink = Sink::try_new(stream).unwrap();
        Arc::new(Player {
            sink,
            stream,
            conn: Arc::new(Mutex::new(conn)),
        })
    }
    pub fn new_stream() -> (OutputStream, OutputStreamHandle) {
        rodio::OutputStream::try_default().unwrap()
    }

    /// unsafe should be fine as long as the application is single threaded
    pub unsafe fn stop(&self) {
        self.sink.stop();
        // `Sink` is no longer usable after stopping it, so we create a new one
        // probably a bug in rodio
        let p: *mut Sink = &self.sink as *const Sink as *mut Sink;
        *p = Sink::try_new(self.stream).unwrap();
    }

    pub async fn play_file(&self, fname: &str) -> Result<()> {
        let file = db::get_file(&*self.conn.lock().await, fname)?;
        self.play_buf(file.data)
    }
    pub fn play_buf(&self, buf: Bytes) -> Result<()> {
        let src = Decoder::new(Cursor::new(buf))?;
        self.sink.append(src);
        Ok(())
    }

    pub fn playtest(&self) {
        // taken from https://docs.rs/rodio
        // Add a dummy source for the sake of the example.
        let source = SineWave::new(880.0)
            .take_duration(Duration::from_secs_f32(1.0))
            .amplify(0.20);
        self.sink.append(source);
    }

    pub async fn db_name<T: Serialize>(
        &self,
        f: impl Fn(&Connection, &str) -> rusqlite::Result<T>,
        name: &str,
    ) -> rusqlite::Result<T> {
        f(&*self.conn.lock().await, name)
    }
}
