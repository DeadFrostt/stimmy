//! UI-independent Stremio protocol, artwork processing, and local storage.

pub mod account;
pub mod artwork;
pub mod client;
pub mod downloads;
pub mod error;
pub mod model;
pub mod playback;
pub mod pool;
pub mod protocol;
pub mod store;
pub mod subtitles;

pub use account::{
    AccountSession, CloudLibraryItem, Profile, StremioUser, WatchProgress, effective_addons,
    keychain,
};
pub use artwork::{ArtworkCache, DEFAULT_CACHE_LIMIT};
pub use client::{StremioClient, genres, shelves};
pub use downloads::{DownloadEvent, DownloadRecord, DownloadState, Downloads, NewDownload};
pub use error::{Error, Result};
pub use model::*;
pub use pool::Pool;
pub use store::{AddonStore, CatalogCache, Library};
pub use subtitles::{Cue, SubtitleTrack, cue_at, language_name};

pub const CINEMETA: &str = "https://v3-cinemeta.strem.io/manifest.json";

pub fn data_directory() -> Result<std::path::PathBuf> {
    let project = directories::ProjectDirs::from("app", "stimmy", "Stimmy GPUI")
        .ok_or_else(|| Error::Storage("no application data folder".into()))?;
    Ok(project.data_dir().to_path_buf())
}
