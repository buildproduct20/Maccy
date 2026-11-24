use crate::import::import_settings;
use crate::macos::Pasteboard;
use crate::models::{Cursor, IMAGE_TYPES};
use crate::repository::Store;
use crate::settings::Settings;
use crate::{CoreError, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Page {
        query: Option<String>,
        cursor: Option<Cursor>,
    },
    Restore {
        id: i64,
    },
    Delete {
        id: i64,
    },
    Clear,
    Settings,
    UpdateSettings {
        settings: Settings,
    },
    ImportSettings {
        plist: String,
        overwrite: Option<bool>,
    },
    Generation,
    Thumbnail {
        id: i64,
        pixels: u32,
    },
}

type Reply = SyncSender<Result<Value>>;
pub struct Worker {
    sender: Option<SyncSender<(Request, Reply)>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn open(root: PathBuf, board: Option<String>) -> Result<Self> {
        let (sender, receiver) = sync_channel(8);
        let (ready, initialization) = sync_channel(1);
        let thread = thread::Builder::new()
            .name("maccy-lite-core".into())
            .spawn(move || {
                let state = objc2::rc::autoreleasepool(|_| State::open(root, board.as_deref()));
                match state {
                    Ok(state) => {
                        if ready.send(Ok(())).is_ok() {
                            state.run(receiver);
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })
            .map_err(|_| CoreError::Storage)?;
        let worker = Self {
            sender: Some(sender),
            thread: Some(thread),
        };
        initialization.recv().map_err(|_| CoreError::Closed)??;
        Ok(worker)
    }

    pub fn run(&self, request: Request) -> Result<Value> {
        let (reply, receive) = sync_channel(1);
        self.sender
            .as_ref()
            .ok_or(CoreError::Closed)?
            .try_send((request, reply))
            .map_err(|err| match err {
                TrySendError::Full(_) => CoreError::Busy,
                TrySendError::Disconnected(_) => CoreError::Closed,
            })?;
        receive.recv().map_err(|_| CoreError::Closed)?
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const THUMB_BUDGET: usize = 8 * 1024 * 1024;

struct State {
    store: Store,
    board: Pasteboard,
    seen: i64,
    generation: u64,
    thumbs: VecDeque<((i64, u32), String)>,
    thumb_bytes: usize,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl State {
    fn open(root: PathBuf, name: Option<&str>) -> Result<Self> {
        let store = Store::open(&root, None, now())?;
        let board = Pasteboard::open(name)?;
        let seen = board.generation();
        Ok(Self {
            store,
            board,
            seen,
            generation: 0,
            thumbs: VecDeque::new(),
            thumb_bytes: 0,
        })
    }

    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn invalidate_thumbs(&mut self) {
        self.thumbs.clear();
        self.thumb_bytes = 0;
    }

    fn run(mut self, receiver: Receiver<(Request, Reply)>) {
        let interval =
            |state: &Self| Duration::from_millis(state.store.settings().poll_interval_ms);
        let mut poll = Instant::now() + interval(&self);
        let mut cleanup = Instant::now() + CLEANUP_INTERVAL;
        loop {
            if Instant::now() >= poll {
                objc2::rc::autoreleasepool(|_| self.poll());
                poll = Instant::now() + interval(&self);
            }
            if Instant::now() >= cleanup {
                if self
                    .store
                    .enforce(now())
                    .is_ok_and(|report| report.deleted_items != 0)
                {
                    self.invalidate_thumbs();
                    self.changed();
                }
                cleanup = Instant::now() + CLEANUP_INTERVAL;
            }
            let deadline = poll.min(cleanup);
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((request, reply)) => {
                    let _ = reply.send(self.request(request));
                }
                Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn poll(&mut self) {
        let observed = self.board.generation();
        if observed == self.seen {
            return;
        }
        match self.board.capture(self.store.settings()) {
            Ok((generation, payload)) => {
                self.seen = generation;
                if let Some(payload) = payload
                    && self
                        .store
                        .ingest(payload, now())
                        .is_ok_and(|id| id.is_some())
                {
                    self.changed();
                }
            }
            Err(CoreError::Busy) => (),
            Err(_) => {
                self.seen = observed;
            }
        }
    }

    fn request(&mut self, request: Request) -> Result<Value> {
        match request {
            Request::Page { query, cursor } => {
                let query = query.as_deref().unwrap_or("");
                if query.len() > 4096 {
                    return Err(CoreError::InvalidInput);
                }
                let page = self.store.page(query, cursor, 50)?;
                Ok(
                    json!({"items": page.items, "next_cursor": page.next_cursor, "generation": self.generation}),
                )
            }
            Request::Generation => {
                Ok(json!({"generation": self.generation, "seen_generation": self.seen}))
            }
            Request::Settings => Ok(serde_json::to_value(self.store.settings())?),
            Request::Restore { id } => {
                let mut payload = self.store.payload(id)?;
                if !payload.text.is_empty()
                    && !payload
                        .representations
                        .iter()
                        .any(|r| r.uti == crate::models::TEXT)
                    && let Some(rich) = payload.representations.iter().find(|r| {
                        matches!(r.uti.as_str(), crate::models::HTML | crate::models::RTF)
                    })
                {
                    payload.representations.push(crate::models::Representation {
                        item_index: rich.item_index,
                        uti: crate::models::TEXT.into(),
                        data: std::mem::take(&mut payload.text).into_bytes(),
                    });
                }
                self.seen = self.board.write(&payload)?;
                self.store.refresh_restored(id, now())?;
                self.changed();
                Ok(json!({"ok": true}))
            }
            Request::Delete { id } => {
                self.store.delete(id)?;
                self.invalidate_thumbs();
                self.changed();
                Ok(json!({"ok": true}))
            }
            Request::Clear => {
                self.store.clear()?;
                self.invalidate_thumbs();
                self.changed();
                Ok(json!({"ok": true}))
            }
            Request::UpdateSettings { mut settings } => {
                settings.import_digest = self.store.settings().import_digest.clone();
                self.store.update_settings(settings, now())?;
                self.invalidate_thumbs();
                self.changed();
                Ok(json!({"ok": true}))
            }
            Request::ImportSettings { plist, overwrite } => {
                if plist.len() > (2 * 1024 * 1024usize).div_ceil(3) * 4 {
                    return Err(CoreError::InvalidInput);
                }
                let bytes = STANDARD
                    .decode(plist)
                    .map_err(|_| CoreError::InvalidInput)?;
                let result =
                    import_settings(&bytes, self.store.settings(), overwrite.unwrap_or(false))?;
                if &result.settings != self.store.settings() {
                    self.store.update_settings(result.settings, now())?;
                    self.invalidate_thumbs();
                    self.changed();
                }
                Ok(json!({"report": result.report}))
            }
            Request::Thumbnail { id, pixels } => {
                let key = (id, pixels);
                // A cached image is usable only while its history row still exists.
                if self.store.contains(id)?
                    && let Some(index) = self.thumbs.iter().position(|(k, _)| *k == key)
                {
                    let entry = self.thumbs.remove(index).ok_or(CoreError::Storage)?;
                    let reply = json!({"png": entry.1});
                    self.thumbs.push_front(entry);
                    return Ok(reply);
                }
                let payload = self.store.payload(id)?;
                let image = payload
                    .representations
                    .iter()
                    .find(|r| IMAGE_TYPES.contains(&r.uti.as_str()))
                    .ok_or(CoreError::Unsupported)?;
                let encoded =
                    STANDARD.encode(crate::native_callbacks::thumbnail(&image.data, pixels)?);
                while self.thumb_bytes + encoded.len() > THUMB_BUDGET {
                    let Some((_, old)) = self.thumbs.pop_back() else {
                        break;
                    };
                    self.thumb_bytes -= old.len();
                }
                let reply = json!({"png": encoded});
                self.thumb_bytes += encoded.len();
                self.thumbs.push_front((key, encoded));
                Ok(reply)
            }
        }
    }
}
