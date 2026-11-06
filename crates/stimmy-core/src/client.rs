//! The blocking Stremio client. Callers run it off the UI thread.

use crate::{
    error::{Error, Result},
    model::{
        Addon, AddonCatalog, Media, SearchResults, Shelf, Stream, StreamResults, StreamSource,
    },
    protocol::{manifest_url, resource_url},
};
use reqwest::blocking::Client;
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::HashSet, thread, time::Duration};

#[derive(Clone)]
pub struct StremioClient {
    pub(crate) http: Client,
    pub(crate) api_base: String,
}

#[derive(Deserialize)]
struct CatalogResponse {
    #[serde(default)]
    metas: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct MetaResponse {
    meta: Media,
}
#[derive(Deserialize)]
struct StreamResponse {
    #[serde(default)]
    streams: Vec<serde_json::Value>,
}

impl StremioClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(25))
                .user_agent(concat!("Stimmy/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| Error::Offline)?,
            api_base: "https://api.strem.io/api".into(),
        })
    }

    /// Points account requests somewhere else, for tests.
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// A failed connection is retried once. Failure statuses aren't: the server
    /// has answered, and those requests can be expensive for a free add-on.
    pub(crate) fn get_json<T: DeserializeOwned>(&self, url: url::Url) -> Result<T> {
        let mut retried = false;
        let response = loop {
            match self.http.get(url.clone()).send() {
                Ok(response) => break response,
                Err(error) if error.is_connect() && !retried => {
                    retried = true;
                    thread::sleep(Duration::from_millis(300));
                }
                Err(error) => return Err(error.into()),
            }
        };
        let response = response.error_for_status()?;
        let bytes = response.bytes()?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Unreadable)
    }

    pub fn install(&self, input: &str) -> Result<Addon> {
        let url = manifest_url(input)?;
        let manifest = self.get_json(url.clone())?;
        Ok(Addon {
            transport_url: url.to_string(),
            manifest,
        })
    }

    /// One malformed entry is skipped rather than failing its whole catalog.
    pub fn catalog(
        &self,
        addon: &Addon,
        catalog: &AddonCatalog,
        extras: &[(&str, &str)],
    ) -> Result<Vec<Media>> {
        let url = resource_url(
            &addon.transport_url,
            "catalog",
            &catalog.kind,
            &catalog.id,
            extras,
        )?;
        let response: CatalogResponse = self.get_json(url)?;
        Ok(response
            .metas
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }

    pub fn search(&self, query: &str, addons: &[Addon]) -> Result<SearchResults> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResults::default());
        }
        let targets: Vec<(&Addon, &AddonCatalog)> = addons
            .iter()
            .flat_map(|addon| {
                addon
                    .manifest
                    .catalogs
                    .iter()
                    .filter(|catalog| catalog.search_only_requires_query())
                    .map(move |catalog| (addon, catalog))
            })
            .take(16)
            .collect();
        let responses: Vec<Result<Vec<Media>>> = thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|(addon, catalog)| {
                    scope.spawn(move || self.catalog(addon, catalog, &[("search", query)]))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(Err(Error::Unreadable)))
                .collect()
        });
        let attempted = responses.len();
        let mut seen = HashSet::new();
        let mut results = SearchResults::default();
        let mut first_error = None;
        for response in responses {
            match response {
                Ok(items) => results
                    .items
                    .extend(items.into_iter().filter(|item| seen.insert(item.key()))),
                Err(error) => {
                    results.failed += 1;
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) if results.failed == attempted => Err(error),
            _ => Ok(results),
        }
    }

    /// Asks each add-on that claims the title, keeping the first good answer.
    pub fn metadata(&self, media: &Media, addons: &[Addon]) -> Result<Media> {
        let mut last_error = Error::Unsupported;
        for addon in addons
            .iter()
            .filter(|addon| addon.manifest.supports("meta", &media.kind, &media.id))
        {
            let response = resource_url(&addon.transport_url, "meta", &media.kind, &media.id, &[])
                .and_then(|url| self.get_json::<MetaResponse>(url));
            match response {
                Ok(response) => return Ok(response.meta),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    /// Asks add-ons concurrently; one that fails is named, others still show.
    pub fn streams(&self, kind: &str, video_id: &str, addons: &[Addon]) -> StreamResults {
        let targets: Vec<&Addon> = addons
            .iter()
            .filter(|addon| addon.manifest.supports("stream", kind, video_id))
            .collect();
        let responses: Vec<Result<Vec<Stream>>> = thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|addon| {
                    scope.spawn(move || {
                        let url =
                            resource_url(&addon.transport_url, "stream", kind, video_id, &[])?;
                        let response: StreamResponse = self.get_json(url)?;
                        Ok(response
                            .streams
                            .into_iter()
                            .filter_map(|value| serde_json::from_value(value).ok())
                            .collect())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(Err(Error::Unreadable)))
                .collect()
        });
        let mut results = StreamResults::default();
        for (addon, response) in targets.iter().zip(responses) {
            match response {
                Ok(streams) => {
                    results
                        .sources
                        .extend(streams.into_iter().map(|stream| StreamSource {
                            addon: addon.manifest.name.clone(),
                            stream,
                        }))
                }
                Err(_) => results.failures.push(addon.manifest.name.clone()),
            }
        }
        results
    }
}

/// Home's shelves in add-on order. Search-only and configuration-gated catalogs
/// are skipped; each add-on is capped so one can't crowd out the rest.
pub fn shelves(addons: &[Addon]) -> Vec<Shelf> {
    let mut shelves = Vec::new();
    for addon in addons {
        let bundled = addon.manifest.id == "com.linvo.cinemeta";
        for catalog in addon
            .manifest
            .catalogs
            .iter()
            .filter(|catalog| catalog.browsable())
            .take(if bundled { 6 } else { 4 })
        {
            let kind = if catalog.kind == "series" {
                "series"
            } else {
                "films"
            };
            let name = catalog.name.as_deref().unwrap_or("Discover");
            shelves.push(Shelf {
                id: format!("{}:{}:{}", addon.transport_url, catalog.kind, catalog.id),
                title: if bundled {
                    format!("{name} {kind}")
                } else {
                    format!("{name} {kind} · {}", addon.manifest.name)
                },
                addon: addon.clone(),
                catalog: catalog.clone(),
            });
        }
    }
    shelves.truncate(16);
    shelves
}

/// Genres offered by the first catalog of a kind that lists any.
pub fn genres(addons: &[Addon], kind: &str) -> Vec<String> {
    addons
        .iter()
        .flat_map(|addon| &addon.manifest.catalogs)
        .find(|catalog| catalog.kind == kind && !catalog.genres().is_empty())
        .map(|catalog| catalog.genres().to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AddonManifest, AddonResource};

    #[test]
    fn manifest_resource_filters_respect_simple_and_detailed_forms() {
        let manifest = AddonManifest {
            id: "x".into(),
            name: "X".into(),
            types: vec!["movie".into()],
            id_prefixes: Some(vec!["tt".into()]),
            resources: vec![
                AddonResource::Simple("meta".into()),
                AddonResource::Detailed {
                    name: "stream".into(),
                    types: Some(vec!["series".into()]),
                    id_prefixes: None,
                },
            ],
            ..Default::default()
        };
        assert!(manifest.supports("meta", "movie", "tt1"));
        assert!(!manifest.supports("meta", "movie", "kitsu:1"));
        assert!(!manifest.supports("meta", "series", "tt1"));
        assert!(manifest.supports("stream", "series", "kitsu:1"));
        assert!(!manifest.supports("stream", "movie", "tt1"));
        assert!(!manifest.supports("subtitles", "movie", "tt1"));
    }

    #[test]
    fn home_shelves_skip_gated_catalogs_and_name_outside_addons() {
        let extra = serde_json::json!({
            "transportUrl": "https://x.example/manifest.json",
            "manifest": {"id":"x","name":"Extra","catalogs":[
                {"id":"a","type":"movie","name":"Picks"},
                {"id":"s","type":"movie","name":"Find","extra":[{"name":"search","isRequired":true}]},
                {"id":"old","type":"series","name":"Gated","extraRequired":["genre"]}
            ]}
        });
        let addons = [Addon::cinemeta(), serde_json::from_value(extra).unwrap()];
        let titles: Vec<_> = shelves(&addons).into_iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            [
                "Popular films",
                "Popular series",
                "Featured films",
                "Featured series",
                "Picks films · Extra"
            ]
        );
        assert_eq!(genres(&addons, "movie")[0], "Action");
    }
}
