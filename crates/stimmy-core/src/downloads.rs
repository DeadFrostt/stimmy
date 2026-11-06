//! Offline downloads.
//!
//! Partial files live on disk under the record's id, so a download survives
//! pausing, losing the network, and quitting the app: it picks up from the bytes
//! it has. Links from add-ons expire, so an expired link is reported for the app
//! to resolve again rather than failing the download.
//!
//! The finished file is named from what its bytes actually are. The player picks
//! its engine from the extension, so a wrong one means a video that won't open.

use crate::{
    account::now,
    error::{Error, Result},
    model::Media,
    store::{atomic_write, read_json, write_json},
    subtitles::{self, Cue, SubtitleTrack},
};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_RANGE, CONTENT_TYPE, RANGE},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;

/// Two at once: more slows each one down, and debrid services often limit it.
const MAX_ACTIVE: usize = 2;
const MAX_ATTEMPTS: u32 = 5;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Downloading,
    Paused,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SavedSubtitle {
    pub lang: String,
    pub file: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DownloadRecord {
    pub id: String,
    pub media: Media,
    pub video_id: String,
    pub episode_label: Option<String>,
    pub addon: String,
    pub stream_name: Option<String>,
    pub binge_group: Option<String>,
    pub url: String,
    pub filename_hint: Option<String>,
    pub state: DownloadState,
    pub written: u64,
    pub total: Option<u64>,
    #[serde(default)]
    pub segments_done: usize,
    #[serde(default)]
    pub segments_total: usize,
    pub file: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub link_expired: bool,
    #[serde(default)]
    pub subtitles: Vec<SavedSubtitle>,
    pub created_at: f64,
}

impl DownloadRecord {
    pub fn fraction(&self) -> f32 {
        if self.state == DownloadState::Complete {
            1.
        } else if self.segments_total > 0 {
            self.segments_done as f32 / self.segments_total as f32
        } else {
            match self.total {
                Some(total) if total > 0 => (self.written as f32 / total as f32).min(1.),
                _ => 0.,
            }
        }
    }
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            DownloadState::Queued | DownloadState::Downloading
        )
    }
}

/// What a new download needs to know.
pub struct NewDownload {
    pub media: Media,
    pub video_id: String,
    pub episode_label: Option<String>,
    pub addon: String,
    pub stream_name: Option<String>,
    pub binge_group: Option<String>,
    pub url: String,
    pub filename_hint: Option<String>,
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadEvent {
    Completed { id: String, title: String },
    LinkExpired { id: String },
    Failed { id: String, title: String },
}

#[derive(Default)]
struct Inner {
    records: Vec<DownloadRecord>,
    running: HashMap<String, Arc<AtomicBool>>,
    speeds: HashMap<String, f64>,
    events: Vec<DownloadEvent>,
    pending_subtitles: HashMap<String, Vec<SubtitleTrack>>,
    last_saved: Option<Instant>,
}

#[derive(Clone)]
pub struct Downloads {
    inner: Arc<Mutex<Inner>>,
    dir: PathBuf,
    http: Client,
}

enum Stop {
    Cancelled,
    Expired,
    Transient(String),
    Fatal(String),
}

fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl Downloads {
    /// Interrupted downloads queue up again, and a finished file that has gone
    /// missing says so rather than offering a Play button that can't work.
    pub fn open(dir: PathBuf) -> Result<Self> {
        // The blocking client applies this timeout to each read, not the whole
        // body, so a long download is fine and only a stalled one fails and retries.
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(45))
            .user_agent(concat!("Stimmy/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Offline)?;
        fs::create_dir_all(&dir)?;
        let mut records: Vec<DownloadRecord> =
            read_json(&dir.join("downloads.json"))?.unwrap_or_default();
        for record in &mut records {
            match record.state {
                DownloadState::Downloading => record.state = DownloadState::Queued,
                DownloadState::Complete => {
                    let present = record
                        .file
                        .as_ref()
                        .is_some_and(|file| dir.join(file).is_file());
                    if !present {
                        record.state = DownloadState::Failed;
                        record.error = Some("The downloaded file is no longer on this Mac.".into());
                    }
                }
                _ => {}
            }
        }
        let this = Self {
            inner: Arc::new(Mutex::new(Inner {
                records,
                ..Default::default()
            })),
            dir,
            http,
        };
        this.save(&mut lock(&this.inner));
        Ok(this)
    }

    fn save(&self, inner: &mut Inner) {
        inner.last_saved = Some(Instant::now());
        // Configured add-on links can carry tokens, so the file stays private.
        let _ = write_json(&self.dir.join("downloads.json"), &inner.records, true);
    }

    pub fn records(&self) -> Vec<DownloadRecord> {
        lock(&self.inner).records.clone()
    }
    pub fn find(&self, media: &Media, video_id: &str) -> Option<DownloadRecord> {
        lock(&self.inner)
            .records
            .iter()
            .find(|r| r.media.key() == media.key() && r.video_id == video_id)
            .cloned()
    }
    pub fn local_file(&self, record: &DownloadRecord) -> Option<PathBuf> {
        (record.state == DownloadState::Complete)
            .then(|| record.file.as_ref().map(|file| self.dir.join(file)))
            .flatten()
            .filter(|path| path.is_file())
    }
    pub fn speed(&self, id: &str) -> Option<f64> {
        lock(&self.inner).speeds.get(id).copied()
    }
    pub fn any_active(&self) -> bool {
        lock(&self.inner)
            .records
            .iter()
            .any(DownloadRecord::is_active)
    }
    pub fn take_events(&self) -> Vec<DownloadEvent> {
        std::mem::take(&mut lock(&self.inner).events)
    }
    /// Subtitles saved with a download, ready for the player.
    pub fn subtitle_tracks(&self, record: &DownloadRecord) -> Vec<SubtitleTrack> {
        record
            .subtitles
            .iter()
            .filter_map(|saved| {
                let url = Url::from_file_path(self.dir.join(&saved.file)).ok()?;
                Some(SubtitleTrack {
                    id: saved.file.clone(),
                    url: url.to_string(),
                    lang: saved.lang.clone(),
                })
            })
            .collect()
    }
    /// Reads a saved subtitle. Only ever from this folder, since track URLs can
    /// also come from add-ons.
    pub fn local_cues(&self, file_url: &str) -> Result<Vec<Cue>> {
        let path = Url::parse(file_url)
            .ok()
            .filter(|url| url.scheme() == "file")
            .and_then(|url| url.to_file_path().ok())
            .ok_or(Error::Unreadable)?;
        let (root, path) = (fs::canonicalize(&self.dir)?, fs::canonicalize(&path)?);
        if !path.starts_with(root) {
            return Err(Error::Unreadable);
        }
        let cues = subtitles::parse(&String::from_utf8_lossy(&fs::read(path)?));
        if cues.is_empty() {
            Err(Error::Unreadable)
        } else {
            Ok(cues)
        }
    }

    pub fn add(&self, new: NewDownload) -> Result<String> {
        let parsed = Url::parse(&new.url).map_err(|_| Error::Unreadable)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(Error::Unreadable);
        }
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = format!(
            "{:x}-{:x}",
            crate::artwork::hash(&format!("{}:{}", new.media.key(), new.video_id)),
            now() as u64 ^ COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let mut media = new.media;
        media.videos.clear();
        let mut inner = lock(&self.inner);
        // A failed attempt at the same video makes way for this one.
        let existing: Vec<String> = inner
            .records
            .iter()
            .filter(|r| r.media.key() == media.key() && r.video_id == new.video_id)
            .map(|r| r.id.clone())
            .collect();
        for old in existing {
            let state = inner.records.iter().find(|r| r.id == old).map(|r| r.state);
            if state != Some(DownloadState::Failed) {
                return Ok(old);
            }
            self.remove_locked(&mut inner, &old);
        }
        inner.pending_subtitles.insert(id.clone(), new.subtitles);
        inner.records.insert(
            0,
            DownloadRecord {
                id: id.clone(),
                media,
                video_id: new.video_id,
                episode_label: new.episode_label,
                addon: new.addon,
                stream_name: new.stream_name,
                binge_group: new.binge_group,
                url: new.url,
                filename_hint: new.filename_hint,
                state: DownloadState::Queued,
                written: 0,
                total: None,
                segments_done: 0,
                segments_total: 0,
                file: None,
                error: None,
                link_expired: false,
                subtitles: vec![],
                created_at: now(),
            },
        );
        self.save(&mut inner);
        drop(inner);
        self.pump();
        Ok(id)
    }

    /// Starts queued downloads, oldest first, up to the limit.
    pub fn pump(&self) {
        let mut inner = lock(&self.inner);
        loop {
            if inner.running.len() >= MAX_ACTIVE {
                return;
            }
            let Some(index) = inner
                .records
                .iter()
                .rposition(|r| r.state == DownloadState::Queued)
            else {
                return;
            };
            inner.records[index].state = DownloadState::Downloading;
            inner.records[index].error = None;
            let id = inner.records[index].id.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            inner.running.insert(id.clone(), cancel.clone());
            self.save(&mut inner);
            let this = self.clone();
            thread::spawn(move || this.run(&id, &cancel));
        }
    }

    fn stop_running(&self, inner: &mut Inner, id: &str) {
        if let Some(cancel) = inner.running.remove(id) {
            cancel.store(true, Ordering::SeqCst);
        }
        inner.speeds.remove(id);
    }

    pub fn pause(&self, id: &str) {
        let mut inner = lock(&self.inner);
        self.stop_running(&mut inner, id);
        if let Some(record) = inner
            .records
            .iter_mut()
            .find(|r| r.id == id && r.is_active())
        {
            record.state = DownloadState::Paused;
        }
        self.save(&mut inner);
        drop(inner);
        self.pump();
    }

    /// Resumes a paused download, or retries a failed one from what it has.
    pub fn resume(&self, id: &str) {
        let mut inner = lock(&self.inner);
        if let Some(record) = inner.records.iter_mut().find(|r| {
            r.id == id && matches!(r.state, DownloadState::Paused | DownloadState::Failed)
        }) {
            record.state = DownloadState::Queued;
            record.error = None;
            record.link_expired = false;
        }
        self.save(&mut inner);
        drop(inner);
        self.pump();
    }

    /// Gives a download a fresh link and restarts it. Partial data is kept only
    /// if it's the same source, since another source's bytes wouldn't line up.
    pub fn refresh_link(&self, id: &str, url: &str, same_source: bool) {
        let mut inner = lock(&self.inner);
        let Some(record) = inner.records.iter_mut().find(|r| r.id == id) else {
            return;
        };
        record.url = url.to_owned();
        record.link_expired = false;
        record.error = None;
        record.state = DownloadState::Queued;
        if !same_source {
            record.written = 0;
            record.total = None;
            record.segments_done = 0;
            record.segments_total = 0;
            let _ = fs::remove_file(self.part_path(id));
        }
        self.save(&mut inner);
        drop(inner);
        self.pump();
    }

    pub fn fail_expired(&self, id: &str) {
        let mut inner = lock(&self.inner);
        if let Some(record) = inner.records.iter_mut().find(|r| r.id == id) {
            record.state = DownloadState::Failed;
            record.link_expired = true;
            record.error =
                Some("The link expired and no replacement was found. Try again later.".into());
        }
        self.save(&mut inner);
    }

    fn remove_locked(&self, inner: &mut Inner, id: &str) {
        self.stop_running(inner, id);
        if let Some(index) = inner.records.iter().position(|r| r.id == id) {
            let record = inner.records.remove(index);
            if let Some(file) = &record.file {
                let _ = fs::remove_file(self.dir.join(file));
            }
            for subtitle in &record.subtitles {
                let _ = fs::remove_file(self.dir.join(&subtitle.file));
            }
        }
        let _ = fs::remove_file(self.part_path(id));
    }

    pub fn remove(&self, id: &str) {
        let mut inner = lock(&self.inner);
        self.remove_locked(&mut inner, id);
        self.save(&mut inner);
        drop(inner);
        self.pump();
    }

    fn part_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.part"))
    }

    fn record(&self, id: &str) -> Option<DownloadRecord> {
        lock(&self.inner)
            .records
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut DownloadRecord)) {
        let mut inner = lock(&self.inner);
        if let Some(record) = inner.records.iter_mut().find(|r| r.id == id) {
            change(record);
        }
        // Progress is worth keeping, but not worth a disk write per chunk.
        if inner
            .last_saved
            .is_none_or(|at| at.elapsed() > Duration::from_secs(2))
        {
            self.save(&mut inner);
        }
    }

    fn run(&self, id: &str, cancel: &Arc<AtomicBool>) {
        self.save_subtitles(id, cancel);
        let mut attempts = 0;
        let outcome = loop {
            let before = self.record(id).map_or(0, |r| r.written);
            match self.transfer(id, cancel) {
                Ok(()) => break Ok(()),
                Err(Stop::Transient(reason)) => {
                    // Progress means the connection works, so the count restarts.
                    if self.record(id).is_some_and(|r| r.written > before) {
                        attempts = 0;
                    }
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        break Err(Stop::Fatal(reason));
                    }
                    let wait = Duration::from_secs(1 << attempts);
                    let started = Instant::now();
                    while started.elapsed() < wait {
                        if cancel.load(Ordering::SeqCst) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                Err(stop) => break Err(stop),
            }
        };
        let mut inner = lock(&self.inner);
        // Pausing or removing has already taken this out of `running`.
        let current = inner
            .running
            .get(id)
            .is_some_and(|flag| Arc::ptr_eq(flag, cancel));
        if !current {
            return;
        }
        inner.running.remove(id);
        inner.speeds.remove(id);
        let Some(record) = inner.records.iter_mut().find(|r| r.id == id) else {
            return;
        };
        let title = record.media.name.clone();
        let event = match outcome {
            Ok(()) => {
                record.state = DownloadState::Complete;
                record.error = None;
                Some(DownloadEvent::Completed {
                    id: id.to_owned(),
                    title,
                })
            }
            Err(Stop::Cancelled) => None,
            Err(Stop::Expired) => {
                record.state = DownloadState::Paused;
                record.link_expired = true;
                Some(DownloadEvent::LinkExpired { id: id.to_owned() })
            }
            Err(Stop::Transient(reason) | Stop::Fatal(reason)) => {
                record.state = DownloadState::Failed;
                record.error = Some(reason);
                Some(DownloadEvent::Failed {
                    id: id.to_owned(),
                    title,
                })
            }
        };
        inner.events.extend(event);
        self.save(&mut inner);
        drop(inner);
        self.pump();
    }

    /// One subtitle per language, so a list with dozens of near-duplicates doesn't
    /// mean dozens of downloads. Failures are fine: the video matters more.
    fn save_subtitles(&self, id: &str, cancel: &AtomicBool) {
        const MAX_LANGUAGES: usize = 12;
        let tracks = lock(&self.inner)
            .pending_subtitles
            .remove(id)
            .unwrap_or_default();
        let mut languages = HashSet::new();
        let mut saved = vec![];
        for track in tracks {
            if saved.len() >= MAX_LANGUAGES || cancel.load(Ordering::SeqCst) {
                break;
            }
            if !languages.insert(track.lang.clone()) {
                continue;
            }
            let Ok(url) = Url::parse(&track.url) else {
                continue;
            };
            if !matches!(url.scheme(), "http" | "https") {
                continue;
            }
            let Ok(response) = self
                .http
                .get(url)
                .send()
                .and_then(Response::error_for_status)
            else {
                continue;
            };
            let mut bytes = Vec::new();
            if response
                .take(8 * 1024 * 1024)
                .read_to_end(&mut bytes)
                .is_err()
            {
                continue;
            }
            if subtitles::parse(&String::from_utf8_lossy(&bytes)).is_empty() {
                continue;
            }
            let file = format!("{id}.{}.srt", sanitize(&track.lang));
            if atomic_write(&self.dir.join(&file), &bytes, false).is_ok() {
                saved.push(SavedSubtitle {
                    lang: track.lang,
                    file,
                });
            }
        }
        if !saved.is_empty() {
            self.update(id, |record| record.subtitles.extend(saved));
        }
    }

    fn transfer(&self, id: &str, cancel: &AtomicBool) -> std::result::Result<(), Stop> {
        let record = self.record(id).ok_or(Stop::Cancelled)?;
        let parsed =
            Url::parse(&record.url).map_err(|_| Stop::Fatal("This link can’t be used.".into()))?;
        if parsed.path().to_ascii_lowercase().ends_with(".m3u8") {
            return self.transfer_hls(id, &parsed, cancel);
        }
        let part = self.part_path(id);
        let offset = fs::metadata(&part).map_or(0, |m| m.len());
        let mut request = self.http.get(parsed.clone());
        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }
        let mut response = request
            .send()
            .map_err(|_| Stop::Transient(network_message()))?;
        let status = response.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            // Everything is here already, unless the file changed underneath us.
            if record.total == Some(offset) {
                return self.finish(id, &part, None);
            }
            let _ = fs::remove_file(&part);
            self.update(id, |r| r.written = 0);
            return Err(Stop::Transient("The source changed. Restarting.".into()));
        }
        if matches!(status.as_u16(), 401 | 403 | 404 | 410) {
            return Err(Stop::Expired);
        }
        if !status.is_success() {
            return Err(Stop::Transient(Error::Server(status.as_u16()).to_string()));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if content_type.contains("mpegurl") {
            return self.transfer_hls(id, &parsed, cancel);
        }
        // Without a partial response the server is sending the whole file again.
        let resumed = status == StatusCode::PARTIAL_CONTENT;
        let total = if resumed {
            response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok())
        } else {
            response.content_length()
        };
        let start = if resumed { offset } else { 0 };
        if resumed
            && let (Some(known), Some(reported)) = (record.total, total)
            && known != reported
        {
            // Same address, different file: the old bytes would corrupt it.
            let _ = fs::remove_file(&part);
            self.update(id, |r| r.written = 0);
            return Err(Stop::Transient("The source changed. Restarting.".into()));
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!resumed)
            .open(&part)
            .map_err(storage_stop)?;
        file.seek(SeekFrom::Start(start)).map_err(storage_stop)?;
        self.update(id, |r| {
            r.written = start;
            if total.is_some() {
                r.total = total;
            }
        });
        let mut written = start;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut meter = Meter::new();
        let mut checked = start > 0;
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = file.flush();
                return Err(Stop::Cancelled);
            }
            let count = match response.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => return Err(Stop::Transient(network_message())),
            };
            if !checked {
                checked = true;
                let head = &buffer[..count];
                if head.starts_with(b"#EXTM3U") {
                    drop(file);
                    let _ = fs::remove_file(&part);
                    return self.transfer_hls(id, &parsed, cancel);
                }
                // Debrid services answer dead links with a page, not an error.
                if looks_like_a_page(head, &content_type) {
                    drop(file);
                    let _ = fs::remove_file(&part);
                    return Err(Stop::Expired);
                }
            }
            file.write_all(&buffer[..count]).map_err(storage_stop)?;
            written += count as u64;
            self.update(id, |r| r.written = written);
            meter.tick(self, id, count);
        }
        file.sync_all().map_err(storage_stop)?;
        drop(file);
        if let Some(total) = total
            && written < total
        {
            return Err(Stop::Transient(network_message()));
        }
        self.finish(id, &part, None)
    }

    /// Unencrypted HLS, joined into one file. Encrypted streams and ones with
    /// separate audio tracks need a real packager, so they say so up front.
    fn transfer_hls(
        &self,
        id: &str,
        url: &Url,
        cancel: &AtomicBool,
    ) -> std::result::Result<(), Stop> {
        let (segments, init) = self.resolve_playlist(url)?;
        let record = self.record(id).ok_or(Stop::Cancelled)?;
        let part = self.part_path(id);
        // Segment boundaries are the only safe place to resume from.
        let done = record.segments_done.min(segments.len());
        let keep = if done > 0 { record.written } else { 0 };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&part)
            .map_err(storage_stop)?;
        file.set_len(keep).map_err(storage_stop)?;
        file.seek(SeekFrom::Start(keep)).map_err(storage_stop)?;
        let extension = hls_extension(segments.first(), init.as_ref());
        let total_segments = segments.len();
        self.update(id, |r| r.segments_total = total_segments);
        let mut written = keep;
        let mut meter = Meter::new();
        if done == 0
            && let Some(init) = init
        {
            let bytes = self.fetch_segment(&init, cancel)?;
            file.write_all(&bytes).map_err(storage_stop)?;
            written += bytes.len() as u64;
        }
        for (index, segment) in segments.iter().enumerate().skip(done) {
            let bytes = self.fetch_segment(segment, cancel)?;
            file.write_all(&bytes).map_err(storage_stop)?;
            written += bytes.len() as u64;
            meter.tick(self, id, bytes.len());
            self.update(id, |r| {
                r.written = written;
                r.segments_done = index + 1;
            });
        }
        file.sync_all().map_err(storage_stop)?;
        drop(file);
        self.finish(id, &part, Some(extension))
    }

    fn fetch_segment(&self, url: &Url, cancel: &AtomicBool) -> std::result::Result<Vec<u8>, Stop> {
        if cancel.load(Ordering::SeqCst) {
            return Err(Stop::Cancelled);
        }
        let response = self
            .http
            .get(url.clone())
            .send()
            .map_err(|_| Stop::Transient(network_message()))?;
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403 | 404 | 410) {
            return Err(Stop::Expired);
        }
        if !status.is_success() {
            return Err(Stop::Transient(Error::Server(status.as_u16()).to_string()));
        }
        let mut bytes = Vec::new();
        let mut body = response;
        body.read_to_end(&mut bytes)
            .map_err(|_| Stop::Transient(network_message()))?;
        Ok(bytes)
    }

    fn fetch_text(&self, url: &Url) -> std::result::Result<String, Stop> {
        let bytes = self.fetch_segment(url, &AtomicBool::new(false))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn resolve_playlist(&self, url: &Url) -> std::result::Result<(Vec<Url>, Option<Url>), Stop> {
        let mut current = url.clone();
        let mut text = self.fetch_text(&current)?;
        if let Playlist::Master(variants) = parse_playlist(&text, &current) {
            let Some(best) = variants
                .into_iter()
                .max_by_key(|v| (!v.separate_audio, v.bandwidth))
            else {
                return Err(Stop::Fatal("This stream has nothing to download.".into()));
            };
            if best.separate_audio {
                return Err(Stop::Fatal(
                    "This stream keeps its audio separately, so it can’t be downloaded yet.".into(),
                ));
            }
            current = best.url;
            text = self.fetch_text(&current)?;
        }
        match parse_playlist(&text, &current) {
            Playlist::Media {
                encrypted: true, ..
            } => Err(Stop::Fatal(
                "This stream is encrypted, so it can’t be downloaded.".into(),
            )),
            Playlist::Media { ended: false, .. } => Err(Stop::Fatal(
                "This is a live stream, so it can’t be downloaded.".into(),
            )),
            Playlist::Media { segments, init, .. } if !segments.is_empty() => Ok((segments, init)),
            _ => Err(Stop::Fatal("This stream has nothing to download.".into())),
        }
    }

    fn finish(
        &self,
        id: &str,
        part: &Path,
        extension: Option<&str>,
    ) -> std::result::Result<(), Stop> {
        let record = self.record(id).ok_or(Stop::Cancelled)?;
        let extension = extension
            .map(str::to_owned)
            .or_else(|| sniff_extension(part))
            .or_else(|| extension_from(record.filename_hint.as_deref()))
            .or_else(|| extension_from(Some(record.url.split('?').next().unwrap_or(""))))
            .unwrap_or_else(|| "mp4".to_owned());
        let name = match &record.episode_label {
            Some(label) => format!("{} {label}", record.media.name),
            None => record.media.name.clone(),
        };
        // The id keeps a re-download from colliding with an older file.
        let short = id.rsplit('-').next().unwrap_or(id);
        let file = format!("{} {short}.{extension}", sanitize(&name));
        fs::rename(part, self.dir.join(&file)).map_err(storage_stop)?;
        let size = fs::metadata(self.dir.join(&file)).map_or(record.written, |m| m.len());
        self.update(id, |r| {
            r.file = Some(file);
            r.written = size;
            r.total = Some(size);
        });
        Ok(())
    }
}

struct Meter {
    since: Instant,
    bytes: usize,
}

impl Meter {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            bytes: 0,
        }
    }
    fn tick(&mut self, downloads: &Downloads, id: &str, count: usize) {
        self.bytes += count;
        let elapsed = self.since.elapsed().as_secs_f64();
        if elapsed >= 1. {
            lock(&downloads.inner)
                .speeds
                .insert(id.to_owned(), self.bytes as f64 / elapsed);
            self.since = Instant::now();
            self.bytes = 0;
        }
    }
}

fn network_message() -> String {
    Error::Offline.to_string()
}

fn storage_stop(error: std::io::Error) -> Stop {
    if error.kind() == std::io::ErrorKind::StorageFull {
        Stop::Fatal("Your Mac is out of space. Free some up, then try again.".into())
    } else {
        Stop::Fatal(Error::from(error).to_string())
    }
}

fn looks_like_a_page(head: &[u8], content_type: &str) -> bool {
    if content_type.starts_with("text/html") || content_type.starts_with("application/json") {
        return true;
    }
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    let head = &head[start..];
    let lower: Vec<u8> = head.iter().take(15).map(u8::to_ascii_lowercase).collect();
    lower.starts_with(b"<!doctype") || lower.starts_with(b"<html") || lower.starts_with(b"{\"")
}

/// From the file's own first bytes, which are more honest than any name.
fn sniff_extension(path: &Path) -> Option<String> {
    let mut head = [0u8; 12];
    let count = File::open(path).ok()?.read(&mut head).ok()?;
    let extension = match &head[..count] {
        [0x1A, 0x45, 0xDF, 0xA3, ..] => "mkv",
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "mp4",
        [0x47, ..] => "ts",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'A', b'V', b'I', b' '] => "avi",
        [b'F', b'L', b'V', ..] => "flv",
        _ => return None,
    };
    Some(extension.to_owned())
}

fn extension_from(name: Option<&str>) -> Option<String> {
    const KNOWN: [&str; 9] = [
        "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "flv",
    ];
    let (_, extension) = name?.rsplit('/').next()?.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    KNOWN.contains(&extension.as_str()).then_some(extension)
}

fn hls_extension(first: Option<&Url>, init: Option<&Url>) -> &'static str {
    let fragmented = init.is_some()
        || first.is_some_and(|url| {
            let path = url.path().to_ascii_lowercase();
            path.ends_with(".m4s") || path.ends_with(".mp4")
        });
    if fragmented { "mp4" } else { "ts" }
}

/// Safe in a file name on any Mac volume, and readable in Finder.
fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_start_matches(['.', ' ']).to_owned();
    if clean.is_empty() {
        "Video".to_owned()
    } else {
        clean.chars().take(120).collect()
    }
}

#[derive(Debug, PartialEq)]
pub struct Variant {
    pub bandwidth: u64,
    pub url: Url,
    pub separate_audio: bool,
}

#[derive(Debug, PartialEq)]
pub enum Playlist {
    Master(Vec<Variant>),
    Media {
        segments: Vec<Url>,
        init: Option<Url>,
        encrypted: bool,
        ended: bool,
    },
}

fn attribute<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let value = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => rest.split(',').next()?,
    };
    Some(value)
}

pub fn parse_playlist(text: &str, base: &Url) -> Playlist {
    let mut variants = vec![];
    let mut pending: Option<(u64, bool)> = None;
    let (mut segments, mut init, mut encrypted, mut ended) = (vec![], None, false, false);
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some((
                attribute(rest, "BANDWIDTH")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                attribute(rest, "AUDIO").is_some(),
            ));
        } else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            encrypted |= attribute(rest, "METHOD").is_some_and(|method| method != "NONE");
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            init = attribute(rest, "URI").and_then(|uri| base.join(uri).ok());
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if !line.is_empty() && !line.starts_with('#') {
            let Ok(url) = base.join(line) else { continue };
            match pending.take() {
                Some((bandwidth, separate_audio)) => variants.push(Variant {
                    bandwidth,
                    url,
                    separate_audio,
                }),
                None => segments.push(url),
            }
        }
    }
    if variants.is_empty() {
        Playlist::Media {
            segments,
            init,
            encrypted,
            ended,
        }
    } else {
        Playlist::Master(variants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_playlists_pick_out_variants_and_media_playlists_their_segments() {
        let base = Url::parse("https://x.example/a/master.m3u8?t=1").unwrap();
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800,RESOLUTION=1x1\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000,AUDIO=\"aac\"\nhigh.m3u8\n";
        let Playlist::Master(variants) = parse_playlist(master, &base) else {
            panic!()
        };
        assert_eq!(variants[0].bandwidth, 800);
        assert!(!variants[0].separate_audio);
        assert!(variants[1].separate_audio);
        assert_eq!(variants[0].url.as_str(), "https://x.example/a/low.m3u8");

        let media = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:6,\none.m4s\n#EXTINF:6,\n/abs/two.m4s\n#EXT-X-ENDLIST\n";
        let Playlist::Media {
            segments,
            init,
            encrypted,
            ended,
        } = parse_playlist(media, &base)
        else {
            panic!()
        };
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].as_str(), "https://x.example/abs/two.m4s");
        assert_eq!(init.unwrap().as_str(), "https://x.example/a/init.mp4");
        assert!(!encrypted && ended);

        let locked = "#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:6,\none.ts\n";
        let Playlist::Media {
            encrypted, ended, ..
        } = parse_playlist(locked, &base)
        else {
            panic!()
        };
        assert!(encrypted && !ended);
    }

    #[test]
    fn file_names_are_safe_and_extensions_come_from_real_bytes() {
        assert_eq!(sanitize("a/b: c?"), "a b c");
        assert_eq!(sanitize("../../etc"), "etc");
        assert_eq!(sanitize("///"), "Video");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x");
        for (bytes, expected) in [
            (&[0x1A, 0x45, 0xDF, 0xA3, 0, 0][..], Some("mkv")),
            (
                &[0, 0, 0, 32, b'f', b't', b'y', b'p', 0, 0, 0, 0][..],
                Some("mp4"),
            ),
            (&[0x47, 0x40, 0, 0][..], Some("ts")),
            (&b"hello"[..], None),
        ] {
            fs::write(&file, bytes).unwrap();
            assert_eq!(sniff_extension(&file).as_deref(), expected);
        }
        assert_eq!(
            extension_from(Some("https://x/Movie.MKV")).as_deref(),
            Some("mkv")
        );
        assert_eq!(extension_from(Some("https://x/stream")), None);
    }

    #[test]
    fn error_pages_are_not_mistaken_for_video() {
        assert!(looks_like_a_page(b"<!DOCTYPE html><html>", ""));
        assert!(looks_like_a_page(b"  <html>", "application/octet-stream"));
        assert!(looks_like_a_page(b"anything", "text/html; charset=utf-8"));
        assert!(!looks_like_a_page(
            &[0x1A, 0x45, 0xDF, 0xA3],
            "video/x-matroska"
        ));
    }
}
