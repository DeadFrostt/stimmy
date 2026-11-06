//! Local persistence: atomic JSON files and the stores built on them.

use crate::{
    error::{Error, Result},
    model::{Addon, Media},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Write to a uniquely named sibling, flush it to disk, then rename over the
/// target, so a crash leaves the old file or the new one and never half of it.
pub fn atomic_write(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Storage("no parent folder".into()))?;
    fs::create_dir_all(parent)?;
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let staging = parent.join(format!(
        ".{}.{}.{unique}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&staging)?;
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        let _ = private;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&staging, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| Error::Storage(format!("{} is damaged", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T, private: bool) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::Storage(e.to_string()))?;
    atomic_write(path, &bytes, private)
}

pub struct Library {
    path: PathBuf,
    items: Vec<Media>,
}

impl Library {
    pub fn open(path: PathBuf) -> Result<Self> {
        let items = read_json(&path)?.unwrap_or_default();
        Ok(Self { path, items })
    }
    pub fn items(&self) -> &[Media] {
        &self.items
    }
    pub fn contains(&self, media: &Media) -> bool {
        self.items.iter().any(|item| item.key() == media.key())
    }
    /// Returns whether the title is now saved. Episode lists are dropped: a
    /// long series would otherwise bloat every save, and details refetch them.
    pub fn toggle(&mut self, media: Media) -> Result<bool> {
        let mut next = self.items.clone();
        let saved = if let Some(index) = next.iter().position(|item| item.key() == media.key()) {
            next.remove(index);
            false
        } else {
            next.insert(
                0,
                Media {
                    videos: vec![],
                    ..media
                },
            );
            true
        };
        write_json(&self.path, &next, false)?;
        self.items = next;
        Ok(saved)
    }
}

/// Add-ons installed on this machine.
/// The file is private to the user: configured add-on URLs can carry tokens.
pub struct AddonStore {
    path: PathBuf,
    installed: Vec<Addon>,
}

impl AddonStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        let installed = read_json(&path)?.unwrap_or_default();
        Ok(Self { path, installed })
    }
    pub fn installed(&self) -> &[Addon] {
        &self.installed
    }
    /// Installing an add-on again updates it in place.
    pub fn install(&mut self, addon: Addon) -> Result<()> {
        let mut next = self.installed.clone();
        if let Some(existing) = next
            .iter_mut()
            .find(|item| item.transport_url == addon.transport_url)
        {
            *existing = addon;
        } else {
            next.push(addon);
        }
        write_json(&self.path, &next, true)?;
        self.installed = next;
        Ok(())
    }
    pub fn remove(&mut self, transport_url: &str) -> Result<()> {
        let next: Vec<Addon> = self
            .installed
            .iter()
            .filter(|item| item.transport_url != transport_url)
            .cloned()
            .collect();
        write_json(&self.path, &next, true)?;
        self.installed = next;
        Ok(())
    }
}

/// Last good catalogs, so Home paints instantly and still works offline.
/// Files are named by hash: a shelf id contains its add-on's URL.
pub struct CatalogCache {
    dir: PathBuf,
}

impl CatalogCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    fn path(&self, key: &str) -> PathBuf {
        self.dir
            .join(format!("{:x}.json", crate::artwork::hash(key)))
    }
    pub fn read(&self, key: &str) -> Option<Vec<Media>> {
        read_json(&self.path(key)).ok().flatten()
    }
    pub fn write(&self, key: &str, items: &[Media]) -> Result<()> {
        write_json(&self.path(key), &items, true)
    }
    pub fn clear(&self) -> Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AddonManifest, Episode};

    fn title() -> Media {
        serde_json::from_value(
            serde_json::json!({"id":"tt0133093", "type":"movie", "name":"The Matrix"}),
        )
        .unwrap()
    }
    fn addon(url: &str, name: &str) -> Addon {
        Addon {
            transport_url: url.into(),
            manifest: AddonManifest {
                id: name.into(),
                name: name.into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn library_round_trips_and_removes_by_type_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut library = Library::open(path.clone()).unwrap();
        assert!(library.toggle(title()).unwrap());
        let mut library = Library::open(path.clone()).unwrap();
        assert!(library.contains(&title()));
        assert!(!library.toggle(title()).unwrap());
        assert!(Library::open(path).unwrap().items().is_empty());
    }

    #[test]
    fn library_does_not_store_episode_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut library = Library::open(path.clone()).unwrap();
        let series = Media {
            kind: "series".into(),
            videos: vec![Episode {
                id: "e".into(),
                ..Default::default()
            }],
            ..title()
        };
        library.toggle(series).unwrap();
        assert!(Library::open(path).unwrap().items()[0].videos.is_empty());
    }

    #[test]
    fn library_written_by_the_previous_build_still_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        fs::write(&path, br#"[{"id":"tt1","type":"movie","name":"Old","poster":null,"background":null,"description":null,"releaseInfo":"1999","runtime":null,"imdbRating":null,"genres":[],"videos":[]}]"#).unwrap();
        let library = Library::open(path).unwrap();
        assert_eq!(library.items()[0].release_info.as_deref(), Some("1999"));
    }

    #[test]
    fn malformed_library_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        fs::write(&path, b"damaged").unwrap();
        assert!(Library::open(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"damaged");
    }

    #[test]
    fn addon_store_updates_in_place_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("addons.json");
        let mut store = AddonStore::open(path.clone()).unwrap();
        store
            .install(addon("https://a/manifest.json", "A"))
            .unwrap();
        store
            .install(addon("https://a/manifest.json", "A2"))
            .unwrap();
        store
            .install(addon("https://b/manifest.json", "B"))
            .unwrap();
        let reopened = AddonStore::open(path.clone()).unwrap();
        let names: Vec<_> = reopened
            .installed()
            .iter()
            .map(|a| a.manifest.name.clone())
            .collect();
        assert_eq!(names, ["A2", "B"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        store.remove("https://a/manifest.json").unwrap();
        assert_eq!(AddonStore::open(path).unwrap().installed().len(), 1);
    }

    #[test]
    fn catalog_cache_round_trips_without_url_in_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CatalogCache::new(dir.path().join("catalogs"));
        let key = "https://secret.example/token/manifest.json:movie:top";
        assert!(cache.read(key).is_none());
        cache.write(key, &[title()]).unwrap();
        assert_eq!(cache.read(key).unwrap(), [title()]);
        for entry in fs::read_dir(dir.path().join("catalogs")).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("secret")
            );
        }
    }

    #[test]
    fn atomic_write_leaves_no_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        atomic_write(&path, b"1", false).unwrap();
        atomic_write(&path, b"2", false).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(fs::read(path).unwrap(), b"2");
    }
}
