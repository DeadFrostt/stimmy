//! Stremio accounts: sign-in, what syncs from the cloud, and per-profile data.
//!
//! The account is read, not written: add-ons and library come down from Stremio,
//! and nothing here overwrites the remote collection. Watch progress recorded on
//! this Mac stays local and merges with the cloud's by recency.

use crate::{
    client::StremioClient,
    error::{Error, Result},
    model::{Addon, Media},
    playback::LastSource,
    store::{Library, read_json, write_json},
};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StremioUser {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub fullname: Option<String>,
}

impl StremioUser {
    pub fn display_name(&self) -> String {
        self.fullname
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| {
                self.email
                    .as_deref()
                    .and_then(|e| e.split('@').next())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "Stremio account".into())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSession {
    pub auth_key: String,
    pub user: StremioUser,
}

fn lenient_number<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<f64, D::Error> {
    let number = match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(value) => value.as_f64().unwrap_or(0.),
        serde_json::Value::String(value) => value.trim().parse().unwrap_or(0.),
        _ => 0.,
    };
    Ok(if number.is_finite() {
        number.max(0.)
    } else {
        0.
    })
}

/// Stremio stores playback time in milliseconds, with a per-series video pointer.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CloudPlaybackState {
    #[serde(default, rename = "timeOffset", deserialize_with = "lenient_number")]
    pub time_offset: f64,
    #[serde(default, deserialize_with = "lenient_number")]
    pub duration: f64,
    #[serde(default, rename = "timesWatched", deserialize_with = "lenient_number")]
    pub times_watched: f64,
    #[serde(default, rename = "video_id")]
    pub video_id: Option<String>,
    #[serde(default, rename = "lastWatched")]
    pub last_watched: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudLibraryItem {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub removed: Option<bool>,
    #[serde(default)]
    pub temp: Option<bool>,
    #[serde(default, rename = "_mtime")]
    pub mtime: Option<String>,
    #[serde(default)]
    pub state: Option<CloudPlaybackState>,
}

impl CloudLibraryItem {
    pub fn media(&self) -> Media {
        Media {
            id: self.id.clone(),
            kind: self.kind.clone(),
            name: self.name.clone(),
            poster: self.poster.clone(),
            background: self.id.starts_with("tt").then(|| {
                format!(
                    "https://images.metahub.space/background/medium/{}/img",
                    self.id
                )
            }),
            logo: self
                .id
                .starts_with("tt")
                .then(|| format!("https://images.metahub.space/logo/medium/{}/img", self.id)),
            ..Default::default()
        }
    }
    pub fn in_library(&self) -> bool {
        self.removed != Some(true) && self.temp != Some(true)
    }
    pub fn continuable(&self) -> bool {
        (self.removed != Some(true) || self.temp == Some(true))
            && self
                .state
                .as_ref()
                .is_some_and(|state| state.time_offset > 0.)
    }
    pub fn progress(&self) -> Option<WatchProgress> {
        let state = self.state.as_ref()?;
        if state.time_offset <= 0. && state.times_watched <= 0. && state.last_watched.is_none() {
            return None;
        }
        let video_id = state
            .video_id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| self.id.clone());
        let parts: Vec<&str> = video_id.split(':').collect();
        let episode_label = (self.kind == "series" && parts.len() >= 3)
            .then(|| {
                let season: u32 = parts[parts.len() - 2].parse().ok()?;
                let episode: u32 = parts[parts.len() - 1].parse().ok()?;
                Some(format!("S{season} E{episode}"))
            })
            .flatten();
        Some(WatchProgress {
            media: self.media(),
            video_id,
            episode_label,
            seconds: state.time_offset / 1000.,
            duration: state.duration / 1000.,
            updated_at: state
                .last_watched
                .as_deref()
                .and_then(parse_iso8601)
                .or_else(|| self.mtime.as_deref().and_then(parse_iso8601))
                .unwrap_or(0.),
        })
    }
}

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SS[.fff]Z`.
pub fn parse_iso8601(text: &str) -> Option<f64> {
    let (date, time) = text.trim().strip_suffix('Z')?.split_once('T')?;
    let mut date = date.split('-');
    let (y, m, d): (i64, i64, i64) = (
        date.next()?.parse().ok()?,
        date.next()?.parse().ok()?,
        date.next()?.parse().ok()?,
    );
    let mut time = time.split(':');
    let (h, min, sec): (f64, f64, f64) = (
        time.next()?.parse().ok()?,
        time.next()?.parse().ok()?,
        time.next()?.parse().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil: Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days as f64 * 86400. + h * 3600. + min * 60. + sec)
}

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |elapsed| elapsed.as_secs_f64())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct WatchProgress {
    pub media: Media,
    pub video_id: String,
    pub episode_label: Option<String>,
    pub seconds: f64,
    pub duration: f64,
    pub updated_at: f64,
}

impl WatchProgress {
    pub fn id(&self) -> String {
        format!("{}:{}", self.media.key(), self.video_id)
    }
    pub fn fraction(&self) -> f64 {
        if self.duration > 0. {
            (self.seconds / self.duration).min(1.)
        } else {
            0.
        }
    }
    /// Near the end counts as finished, so a title doesn't resume at the credits.
    pub fn resumable(&self) -> bool {
        self.seconds > 0. && self.fraction() < 0.95
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ProfileData {
    hidden_cloud: Vec<String>,
    history: Vec<WatchProgress>,
    cloud: Vec<CloudLibraryItem>,
    account_addons: Vec<Addon>,
    last_sources: HashMap<String, LastSource>,
    last_source: Option<LastSource>,
}

/// Everything belonging to one person on this Mac: guest or a signed-in account.
pub struct Profile {
    path: PathBuf,
    library: Library,
    data: ProfileData,
}

impl Profile {
    pub fn open(dir: PathBuf) -> Result<Self> {
        Ok(Self {
            library: Library::open(dir.join("library.json"))?,
            data: read_json(&dir.join("profile.json"))?.unwrap_or_default(),
            path: dir.join("profile.json"),
        })
    }
    fn save(&self) -> Result<()> {
        write_json(&self.path, &self.data, true)
    }
    pub fn library_items(&self) -> Vec<Media> {
        let mut seen: HashSet<String> = self.data.hidden_cloud.iter().cloned().collect();
        self.library
            .items()
            .iter()
            .cloned()
            .chain(
                self.data
                    .cloud
                    .iter()
                    .filter(|i| i.in_library())
                    .map(|i| i.media()),
            )
            .filter(|media| seen.insert(media.key()))
            .collect()
    }
    pub fn contains(&self, media: &Media) -> bool {
        self.library_items()
            .iter()
            .any(|item| item.key() == media.key())
    }
    /// Removing a title that came from the cloud hides it here, and leaves the
    /// account's library as it is.
    pub fn toggle(&mut self, media: Media) -> Result<bool> {
        let key = media.key();
        if self.contains(&media) {
            if self.library.contains(&media) {
                self.library.toggle(media)?;
            }
            if self.data.cloud.iter().any(|item| item.media().key() == key) {
                self.data.hidden_cloud.push(key);
                self.save()?;
            }
            Ok(false)
        } else {
            self.library.toggle(media)?;
            self.data.hidden_cloud.retain(|hidden| *hidden != key);
            self.save()?;
            Ok(true)
        }
    }
    pub fn record(&mut self, mut progress: WatchProgress) -> Result<()> {
        if !progress.seconds.is_finite()
            || !progress.duration.is_finite()
            || progress.seconds <= 0.
            || progress.duration <= 0.
        {
            return Ok(());
        }
        progress.media.videos.clear();
        let id = progress.id();
        self.data.history.retain(|entry| entry.id() != id);
        self.data.history.insert(0, progress);
        self.data.history.truncate(100);
        self.save()
    }
    fn all_progress(&self) -> impl Iterator<Item = WatchProgress> + '_ {
        self.data
            .history
            .iter()
            .cloned()
            .chain(self.data.cloud.iter().filter_map(|item| item.progress()))
    }
    pub fn progress_for(&self, video_id: &str, media: &Media) -> Option<WatchProgress> {
        self.all_progress()
            .filter(|entry| entry.video_id == video_id && entry.media.key() == media.key())
            .max_by(|a, b| a.updated_at.total_cmp(&b.updated_at))
    }
    /// The most recently watched video of a title, finished or not.
    pub fn latest_for(&self, media: &Media) -> Option<WatchProgress> {
        self.all_progress()
            .filter(|entry| entry.media.key() == media.key())
            .max_by(|a, b| a.updated_at.total_cmp(&b.updated_at))
    }
    /// Remembered once a source has actually shown a picture.
    pub fn remember_source(&mut self, media: &Media, source: LastSource) -> Result<()> {
        self.data.last_sources.insert(media.key(), source.clone());
        self.data.last_source = Some(source);
        self.save()
    }
    /// This title's source, or else whatever was used last for anything.
    pub fn last_source_for(&self, media: &Media) -> Option<&LastSource> {
        self.data
            .last_sources
            .get(&media.key())
            .or(self.data.last_source.as_ref())
    }
    /// The latest unfinished video per title, newest first.
    pub fn continue_watching(&self) -> Vec<WatchProgress> {
        let eligible: HashSet<String> = self
            .data
            .cloud
            .iter()
            .filter(|item| item.continuable())
            .map(|item| item.media().key())
            .collect();
        let local: HashSet<String> = self
            .data
            .history
            .iter()
            .map(|entry| entry.media.key())
            .collect();
        let mut all: Vec<WatchProgress> = self.all_progress().collect();
        all.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        let mut seen = HashSet::new();
        all.into_iter()
            .filter(|entry| seen.insert(entry.media.key()))
            .filter(|entry| {
                let key = entry.media.key();
                entry.resumable() && (local.contains(&key) || eligible.contains(&key))
            })
            .collect()
    }
    pub fn set_cloud(&mut self, items: Vec<CloudLibraryItem>) -> Result<()> {
        self.data.cloud = items;
        self.save()
    }
    pub fn set_account_addons(&mut self, addons: Vec<Addon>) -> Result<()> {
        self.data.account_addons = addons;
        self.save()
    }
    pub fn account_addons(&self) -> &[Addon] {
        &self.data.account_addons
    }
}

/// Account add-ons first, then this Mac's, with Cinemeta always available.
pub fn effective_addons(account: &[Addon], local: &[Addon]) -> Vec<Addon> {
    let mut seen = HashSet::new();
    let mut combined: Vec<Addon> = account
        .iter()
        .chain(local)
        .filter(|addon| seen.insert(addon.transport_url.clone()))
        .cloned()
        .collect();
    if !combined
        .iter()
        .any(|addon| addon.manifest.id == "com.linvo.cinemeta")
    {
        combined.insert(0, Addon::cinemeta());
    }
    combined
}

#[derive(Deserialize)]
struct Envelope<T> {
    result: Option<T>,
    error: Option<ApiError>,
}
#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}
#[derive(Deserialize)]
struct AddonCollection {
    #[serde(default)]
    addons: Vec<serde_json::Value>,
}

impl StremioClient {
    fn api<T: DeserializeOwned>(&self, method: &str, body: serde_json::Value) -> Result<T> {
        let url = format!("{}/{method}", self.api_base);
        let bytes = self
            .http
            .post(url)
            .json(&body)
            .send()?
            .error_for_status()?
            .bytes()?;
        let envelope: Envelope<T> =
            serde_json::from_slice(&bytes).map_err(|_| Error::Unreadable)?;
        if let Some(error) = envelope.error {
            return Err(Error::Account(error.message.unwrap_or_else(|| {
                "Stremio couldn’t complete the request.".into()
            })));
        }
        envelope.result.ok_or(Error::Unreadable)
    }
    pub fn login(&self, email: &str, password: &str) -> Result<AccountSession> {
        self.api(
            "login",
            serde_json::json!({"email": email.trim(), "password": password}),
        )
    }
    /// Reads the account's collection. `addFromURL` stays empty: nothing is written.
    pub fn account_addons(&self, auth_key: &str) -> Result<Vec<Addon>> {
        let result: AddonCollection = self.api(
            "addonCollectionGet",
            serde_json::json!({"authKey": auth_key, "update": true, "addFromURL": []}),
        )?;
        Ok(result
            .addons
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }
    /// One malformed item is skipped rather than losing the whole library.
    pub fn cloud_library(&self, auth_key: &str) -> Result<Vec<CloudLibraryItem>> {
        let result: Vec<serde_json::Value> = self.api(
            "datastoreGet",
            serde_json::json!({"authKey": auth_key, "collection": "libraryItem", "ids": [], "all": true}),
        )?;
        Ok(result
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }
}

/// Sessions live in the platform's secret store, never in a JSON file.
pub mod keychain {
    use super::{AccountSession, Error, Result};

    const ACCOUNT: &str = "sessions";
    pub const SERVICE: &str = "app.stimmy.gpui.accounts";

    #[cfg(target_os = "macos")]
    pub fn read(service: &str) -> Result<Vec<AccountSession>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(service, ACCOUNT) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| Error::Storage("saved accounts couldn’t be read".into())),
            Err(error) if error.code() == -25300 => Ok(vec![]), // errSecItemNotFound
            Err(error) => Err(Error::Storage(format!(
                "saved accounts couldn’t be read from Keychain ({}, code {})",
                error,
                error.code()
            ))),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn write(service: &str, sessions: &[AccountSession]) -> Result<()> {
        use security_framework::passwords::set_generic_password;
        let bytes = serde_json::to_vec(sessions).map_err(|e| Error::Storage(e.to_string()))?;
        set_generic_password(service, ACCOUNT, &bytes).map_err(|error| {
            Error::Storage(format!(
                "Keychain refused to save it ({}, code {})",
                error,
                error.code()
            ))
        })
    }

    /// The Secret Service (GNOME Keyring, KWallet) over D-Bus. Where no provider
    /// is running, sessions fall back to a private file in the data folder: the
    /// same protection a browser gives its cookies, and better than losing the
    /// account on a compositor that ships no keyring.
    #[cfg(not(target_os = "macos"))]
    mod secret_service {
        use super::{ACCOUNT, AccountSession, Error, Result};
        use keyring::Entry;
        use std::path::PathBuf;

        fn fallback() -> Result<PathBuf> {
            Ok(crate::data_directory()?.join("accounts.json"))
        }

        fn decode(bytes: &[u8]) -> Result<Vec<AccountSession>> {
            serde_json::from_slice(bytes)
                .map_err(|_| Error::Storage("saved accounts couldn’t be read".into()))
        }

        fn read_fallback() -> Result<Vec<AccountSession>> {
            match std::fs::read(fallback()?) {
                Ok(bytes) => decode(&bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
                Err(error) => Err(error.into()),
            }
        }

        pub fn read(service: &str) -> Result<Vec<AccountSession>> {
            let Ok(entry) = Entry::new(service, ACCOUNT) else {
                return read_fallback();
            };
            match entry.get_secret() {
                Ok(bytes) => decode(&bytes),
                // No provider, or nothing stored yet: either way the file is
                // the only other place sessions can be.
                Err(_) => read_fallback(),
            }
        }

        pub fn write(service: &str, sessions: &[AccountSession]) -> Result<()> {
            let bytes = serde_json::to_vec(sessions).map_err(|e| Error::Storage(e.to_string()))?;
            if let Ok(entry) = Entry::new(service, ACCOUNT)
                && entry.set_secret(&bytes).is_ok()
            {
                return Ok(());
            }
            crate::store::atomic_write(&fallback()?, &bytes, true)
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub use secret_service::{read, write};
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[{"_id":"tt123","type":"series","name":"A series","removed":false,"temp":false,"_mtime":"2026-10-15T12:00:00.000Z","state":{"timeOffset":125000,"duration":1500000,"video_id":"tt123:2:3","lastWatched":"2026-10-15T13:00:00.000Z"}},{"_id":"tt456","type":"movie","name":"Temporary movie","removed":true,"temp":true,"state":{"timeOffset":1,"duration":0}},{"_id":"tt789","type":"movie","name":"Removed movie","removed":true,"temp":false,"state":{"timeOffset":"5000","duration":50000}}]"#;

    fn cloud() -> Vec<CloudLibraryItem> {
        serde_json::from_str(FIXTURE).unwrap()
    }
    fn title(id: &str) -> Media {
        Media {
            id: id.into(),
            kind: "movie".into(),
            name: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn cloud_items_decode_milliseconds_and_episode_pointers() {
        let items = cloud();
        let progress = items[0].progress().unwrap();
        assert_eq!(progress.seconds, 125.);
        assert_eq!(progress.duration, 1500.);
        assert_eq!(progress.video_id, "tt123:2:3");
        assert_eq!(progress.episode_label.as_deref(), Some("S2 E3"));
        assert_eq!(items.iter().filter(|i| i.in_library()).count(), 1);
        assert_eq!(items.iter().filter(|i| i.continuable()).count(), 2);
        assert_eq!(
            items[2].state.as_ref().unwrap().time_offset,
            5000.,
            "strings decode"
        );
    }

    #[test]
    fn iso_dates_parse_to_epoch_seconds() {
        assert_eq!(parse_iso8601("1970-01-02T00:00:00.000Z"), Some(86400.));
        assert_eq!(parse_iso8601("2026-10-15T13:00:00Z"), Some(1792069200.));
        assert_eq!(parse_iso8601("nonsense"), None);
    }

    #[test]
    fn profile_merges_cloud_and_local_and_prefers_newer_progress() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(dir.path().into()).unwrap();
        profile.set_cloud(cloud()).unwrap();
        assert_eq!(profile.library_items().len(), 1);
        assert_eq!(profile.continue_watching().len(), 2);

        let media = profile.library_items()[0].clone();
        let newer = WatchProgress {
            media: media.clone(),
            video_id: "tt123:2:4".into(),
            episode_label: Some("S2 E4".into()),
            seconds: 10.,
            duration: 1500.,
            updated_at: now(),
        };
        profile.record(newer).unwrap();
        let reopened = Profile::open(dir.path().into()).unwrap();
        assert_eq!(reopened.continue_watching()[0].video_id, "tt123:2:4");
        assert_eq!(
            reopened.progress_for("tt123:2:3", &media).unwrap().seconds,
            125.
        );
    }

    #[test]
    fn removing_a_cloud_title_hides_it_here_and_saving_brings_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(dir.path().into()).unwrap();
        profile.set_cloud(cloud()).unwrap();
        let media = profile.library_items()[0].clone();
        assert!(!profile.toggle(media.clone()).unwrap());
        assert!(profile.library_items().is_empty());
        assert!(profile.toggle(media).unwrap());
        assert_eq!(profile.library_items().len(), 1);
    }

    #[test]
    fn finished_titles_do_not_offer_to_resume_and_bad_progress_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(dir.path().into()).unwrap();
        let entry = |seconds: f64, duration: f64| WatchProgress {
            media: title("tt1"),
            video_id: "tt1".into(),
            episode_label: None,
            seconds,
            duration,
            updated_at: now(),
        };
        profile.record(entry(f64::NAN, 10.)).unwrap();
        profile.record(entry(0., 10.)).unwrap();
        assert!(profile.continue_watching().is_empty());
        profile.record(entry(99., 100.)).unwrap();
        assert!(profile.continue_watching().is_empty(), "finished");
        profile.record(entry(50., 100.)).unwrap();
        assert_eq!(profile.continue_watching().len(), 1);
    }

    #[test]
    fn the_last_source_is_remembered_per_title_and_overall() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(dir.path().into()).unwrap();
        let source = LastSource {
            addon: "A".into(),
            name: Some("1080p".into()),
            binge_group: None,
        };
        assert!(profile.last_source_for(&title("tt1")).is_none());
        profile
            .remember_source(&title("tt1"), source.clone())
            .unwrap();
        let reopened = Profile::open(dir.path().into()).unwrap();
        assert_eq!(reopened.last_source_for(&title("tt1")), Some(&source));
        assert_eq!(
            reopened.last_source_for(&title("tt2")),
            Some(&source),
            "a new title starts with it"
        );
    }

    #[test]
    fn effective_addons_dedupe_and_keep_cinemeta() {
        let extra = Addon {
            transport_url: "https://x/manifest.json".into(),
            manifest: Default::default(),
        };
        let addons = effective_addons(std::slice::from_ref(&extra), std::slice::from_ref(&extra));
        assert_eq!(addons.len(), 2);
        assert_eq!(addons[0].manifest.name, "Cinemeta");
        let cinemeta = Addon::cinemeta();
        assert_eq!(
            effective_addons(std::slice::from_ref(&cinemeta), &[]).len(),
            1
        );
    }
}
