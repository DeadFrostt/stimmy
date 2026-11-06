//! Stremio protocol models.
//!
//! Add-ons are third-party servers, so decoding is forgiving: numbers arrive as
//! strings and strings as numbers, arrays arrive as null, and one malformed
//! title must never take a whole catalog with it.

use serde::{Deserialize, Deserializer, Serialize};

fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn lenient_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(value) => value.as_u64().and_then(|n| u32::try_from(n).ok()),
        serde_json::Value::String(value) => value.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_strings<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(values) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
        serde_json::Value::String(value) if !value.is_empty() => vec![value],
        _ => vec![],
    })
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default, deserialize_with = "lenient_string")]
    pub poster: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub background: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub logo: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub release_info: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub runtime: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub imdb_rating: Option<String>,
    #[serde(default, deserialize_with = "lenient_strings")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "lenient_strings")]
    pub cast: Vec<String>,
    #[serde(default, deserialize_with = "lenient_strings")]
    pub director: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub videos: Vec<Episode>,
}

impl Media {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }
    pub fn year(&self) -> &str {
        self.release_info.as_deref().unwrap_or("")
    }
    pub fn is_series(&self) -> bool {
        self.kind == "series"
    }
    pub fn kind_label(&self) -> &'static str {
        if self.is_series() { "Series" } else { "Film" }
    }
    /// The IMDb page, only for an id that is exactly an IMDb title id.
    pub fn imdb_url(&self) -> Option<String> {
        let digits = self.id.strip_prefix("tt")?;
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| format!("https://www.imdb.com/title/{}/", self.id))
    }
    /// Seasons in order, with specials (season 0) last.
    pub fn seasons(&self) -> Vec<u32> {
        let mut seasons: Vec<u32> = self.videos.iter().filter_map(|v| v.season).collect();
        seasons.sort_unstable_by_key(|season| (*season == 0, *season));
        seasons.dedup();
        seasons
    }
    pub fn episodes(&self, season: u32) -> Vec<&Episode> {
        let mut episodes: Vec<&Episode> = self
            .videos
            .iter()
            .filter(|v| v.season == Some(season))
            .collect();
        episodes.sort_by_key(|v| v.number());
        episodes
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Episode {
    pub id: String,
    /// Cinemeta names episodes with `name`; other add-ons use `title`.
    #[serde(
        default,
        deserialize_with = "lenient_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode: Option<u32>,
    /// Some add-ons send `number` in place of `episode`.
    #[serde(
        default,
        deserialize_with = "lenient_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub number: Option<u32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub overview: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub thumbnail: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub released: Option<String>,
}

impl Episode {
    pub fn number(&self) -> u32 {
        self.episode.or(self.number).unwrap_or(0)
    }
    pub fn display_title(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.title.clone())
            .unwrap_or_else(|| format!("Episode {}", self.number()))
    }
    pub fn synopsis(&self) -> Option<&str> {
        self.overview.as_deref().or(self.description.as_deref())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum AddonResource {
    Simple(String),
    Detailed {
        name: String,
        #[serde(default)]
        types: Option<Vec<String>>,
        #[serde(default, rename = "idPrefixes")]
        id_prefixes: Option<Vec<String>>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExtra {
    pub name: String,
    #[serde(default)]
    pub is_required: Option<bool>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddonCatalog {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub extra: Vec<CatalogExtra>,
    #[serde(default)]
    pub extra_supported: Vec<String>,
    #[serde(default)]
    pub extra_required: Vec<String>,
}

impl AddonCatalog {
    pub fn searchable(&self) -> bool {
        self.extra.iter().any(|extra| extra.name == "search")
            || self.extra_supported.iter().any(|name| name == "search")
    }
    /// Can be requested with no extras, so it can be a shelf on Home.
    pub fn browsable(&self) -> bool {
        !self
            .extra
            .iter()
            .any(|extra| extra.is_required == Some(true))
            && self.extra_required.is_empty()
    }
    /// Searchable with only a search term: nothing else is required.
    pub fn search_only_requires_query(&self) -> bool {
        self.searchable()
            && !self
                .extra
                .iter()
                .any(|extra| extra.is_required == Some(true) && extra.name != "search")
            && self.extra_required.iter().all(|name| name == "search")
    }
    pub fn genres(&self) -> &[String] {
        self.extra
            .iter()
            .find(|extra| extra.name == "genre")
            .and_then(|extra| extra.options.as_deref())
            .unwrap_or(&[])
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddonManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub resources: Vec<AddonResource>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub id_prefixes: Option<Vec<String>>,
    #[serde(default)]
    pub catalogs: Vec<AddonCatalog>,
}

impl AddonManifest {
    /// A simple resource inherits the manifest's types and id prefixes. A
    /// detailed resource's own prefixes replace them, and if absent mean "any".
    pub fn supports(&self, resource: &str, kind: &str, id: &str) -> bool {
        self.resources.iter().any(|candidate| {
            let (name, types, prefixes) = match candidate {
                AddonResource::Simple(name) => (name, &self.types, &self.id_prefixes),
                AddonResource::Detailed {
                    name,
                    types,
                    id_prefixes,
                } => (name, types.as_ref().unwrap_or(&self.types), id_prefixes),
            };
            name == resource
                && (types.is_empty() || types.iter().any(|t| t == kind))
                && prefixes
                    .as_ref()
                    .is_none_or(|list| list.iter().any(|p| id.starts_with(p.as_str())))
        })
    }
    pub fn provides_streams(&self) -> bool {
        self.resources.iter().any(|resource| match resource {
            AddonResource::Simple(name) | AddonResource::Detailed { name, .. } => name == "stream",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Addon {
    pub transport_url: String,
    pub manifest: AddonManifest,
}

impl Addon {
    /// The built-in source of public metadata, available with no network.
    pub fn cinemeta() -> Self {
        serde_json::from_str(include_str!("cinemeta.json")).expect("bundled Cinemeta manifest")
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHeaders {
    #[serde(default)]
    pub request: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamHints {
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub binge_group: Option<String>,
    #[serde(default)]
    pub proxy_headers: Option<ProxyHeaders>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    #[serde(default, deserialize_with = "lenient_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub external_url: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub info_hash: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub yt_id: Option<String>,
    #[serde(default)]
    pub behavior_hints: Option<StreamHints>,
    #[serde(default)]
    pub subtitles: Vec<crate::subtitles::SubtitleTrack>,
}

fn http_url(value: &str) -> Option<&str> {
    let parsed = url::Url::parse(value).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(value)
}

impl Stream {
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or("Stream")
    }
    pub fn binge_group(&self) -> Option<&str> {
        self.behavior_hints.as_ref()?.binge_group.as_deref()
    }
    pub fn filename(&self) -> Option<&str> {
        self.behavior_hints.as_ref()?.filename.as_deref()
    }
    pub fn detail(&self) -> &str {
        self.description
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or("")
    }
    pub fn needs_proxy(&self) -> bool {
        self.behavior_hints
            .as_ref()
            .and_then(|hints| hints.proxy_headers.as_ref())
            .and_then(|headers| headers.request.as_ref())
            .is_some_and(|request| !request.is_empty())
    }
    /// What this client can honestly do with the stream today.
    pub fn action(&self) -> StreamAction<'_> {
        if let Some(url) = self.url.as_deref().and_then(http_url) {
            if self.needs_proxy() {
                StreamAction::Unsupported("Needs custom request headers")
            } else {
                StreamAction::Direct(url)
            }
        } else if let Some(url) = self.external_url.as_deref().and_then(http_url) {
            StreamAction::Web(url)
        } else if self.info_hash.is_some() {
            StreamAction::Unsupported("Torrent sources aren’t supported yet")
        } else if self.yt_id.is_some() {
            StreamAction::Unsupported("YouTube sources aren’t supported yet")
        } else {
            StreamAction::Unsupported("Unknown source type")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamAction<'a> {
    /// A direct media URL a local player can open.
    Direct(&'a str),
    /// A page the add-on wants opened in the browser.
    Web(&'a str),
    Unsupported(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamSource {
    pub addon: String,
    pub stream: Stream,
}

/// Successful sources in add-on order, with the add-ons that failed named.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamResults {
    pub sources: Vec<StreamSource>,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub items: Vec<Media>,
    /// Catalogs that failed while others succeeded.
    pub failed: usize,
}

/// One shelf on Home: a catalog that can be requested with no extras.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub id: String,
    pub title: String,
    pub addon: Addon,
    pub catalog: AddonCatalog,
}
