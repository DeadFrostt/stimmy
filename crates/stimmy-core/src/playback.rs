//! Playback decisions that don't depend on an engine, kept here to be testable.

use crate::{
    account::WatchProgress,
    model::{Episode, Media, StreamAction, StreamResults, StreamSource},
};
use serde::{Deserialize, Serialize};

/// Containers AVFoundation can't open, which go straight to the compatibility
/// engine rather than failing first.
pub fn prefers_compatibility(url: &str, filename: Option<&str>) -> bool {
    const COMPATIBILITY: [&str; 6] = ["mkv", "avi", "webm", "m2ts", "ts", "flv"];
    let extension = |text: &str| -> Option<String> {
        let name = text.rsplit('/').next()?;
        let (_, extension) = name.rsplit_once('.')?;
        Some(extension.to_ascii_lowercase())
    };
    let path = url::Url::parse(url)
        .map(|u| u.path().to_owned())
        .unwrap_or_default();
    [filename.and_then(extension), extension(&path)]
        .iter()
        .flatten()
        .any(|found| COMPATIBILITY.contains(&found.as_str()))
}

/// `9:05`, or `1:02:09` past an hour.
pub fn format_clock(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.) as u64
    } else {
        0
    };
    let (hours, minutes, secs) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// `34 min left` for a progress card.
pub fn time_left(seconds: f64, duration: f64) -> String {
    let minutes = ((duration - seconds).max(0.) / 60.).ceil() as u64;
    match minutes {
        0 => "Finishing".into(),
        1..=59 => format!("{minutes} min left"),
        _ => format!("{}h {}m left", minutes / 60, minutes % 60),
    }
}

/// The episode after this one: next in the season, then the next season's first.
/// Specials are never "next".
pub fn next_episode<'a>(media: &'a Media, video_id: &str) -> Option<&'a Episode> {
    let current = media.videos.iter().find(|video| video.id == video_id)?;
    let season = current.season?;
    let number = current.number();
    media
        .videos
        .iter()
        .filter(|video| video.season.is_some_and(|s| s > 0))
        .filter(|video| {
            let s = video.season.unwrap_or(0);
            s > season || (s == season && video.number() > number)
        })
        .min_by_key(|video| (video.season.unwrap_or(0), video.number()))
}

/// The source that last played, so the next Play can go straight back to it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct LastSource {
    pub addon: String,
    pub name: Option<String>,
    pub binge_group: Option<String>,
}

impl LastSource {
    pub fn of(source: &StreamSource) -> Self {
        Self {
            addon: source.addon.clone(),
            name: source.stream.name.clone(),
            binge_group: source.stream.binge_group().map(str::to_owned),
        }
    }
}

/// Playable sources, best guess first, for playing with no menu in the way. A
/// binge group is the add-on saying "these are the same source across episodes";
/// after that comes the same add-on and label, then the same add-on. Otherwise
/// the add-ons' own order stands, since they rank their results.
pub fn rank_sources(results: &StreamResults, last: Option<&LastSource>) -> Vec<StreamSource> {
    let score = |source: &StreamSource| -> u8 {
        let Some(last) = last else { return 0 };
        if source.stream.binge_group().is_some()
            && source.stream.binge_group() == last.binge_group.as_deref()
        {
            3
        } else if source.addon == last.addon && source.stream.name == last.name {
            2
        } else if source.addon == last.addon {
            1
        } else {
            0
        }
    };
    let mut ranked: Vec<StreamSource> = results
        .sources
        .iter()
        .filter(|source| matches!(source.stream.action(), StreamAction::Direct(_)))
        .cloned()
        .collect();
    ranked.sort_by_key(|source| std::cmp::Reverse(score(source)));
    ranked
}

/// What Play means for a title: carry on with what was being watched, go to the
/// next episode if that finished, or start at the beginning.
pub fn up_next(media: &Media, latest: Option<&WatchProgress>) -> Option<String> {
    if !media.is_series() {
        return Some(media.id.clone());
    }
    if let Some(latest) = latest {
        if latest.resumable() {
            return Some(latest.video_id.clone());
        }
        if let Some(next) = next_episode(media, &latest.video_id) {
            return Some(next.id.clone());
        }
        if !media.videos.is_empty() || latest.fraction() < 0.95 {
            return Some(latest.video_id.clone());
        }
    }
    let first = media.seasons().into_iter().find(|season| *season > 0)?;
    media
        .episodes(first)
        .first()
        .map(|episode| episode.id.clone())
}

/// `S1 E2  ·  Title` when the episode is known, or from the id when it isn't.
pub fn episode_label(media: &Media, video_id: &str) -> Option<String> {
    if !media.is_series() {
        return None;
    }
    if let Some(episode) = media.videos.iter().find(|video| video.id == video_id) {
        return Some(format!(
            "S{} E{}  ·  {}",
            episode.season.unwrap_or(0),
            episode.number(),
            episode.display_title()
        ));
    }
    let season = season_of(video_id)?;
    let number = video_id.rsplit(':').next()?;
    Some(format!("S{season} E{number}"))
}

/// The season a `id:season:episode` video belongs to.
pub fn season_of(video_id: &str) -> Option<u32> {
    let parts: Vec<&str> = video_id.split(':').collect();
    (parts.len() >= 3)
        .then(|| parts[parts.len() - 2].parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(id: &str, season: u32, number: u32) -> Episode {
        Episode {
            id: id.into(),
            season: Some(season),
            episode: Some(number),
            ..Default::default()
        }
    }

    #[test]
    fn containers_avfoundation_cannot_open_are_routed_to_compatibility() {
        assert!(prefers_compatibility("https://x/a/movie.MKV?token=1", None));
        assert!(prefers_compatibility(
            "https://x/stream/abc",
            Some("Movie.2024.mkv")
        ));
        assert!(prefers_compatibility("https://x/a.ts", None));
        assert!(!prefers_compatibility("https://x/a.mp4", None));
        assert!(!prefers_compatibility("https://x/master.m3u8", None));
        assert!(!prefers_compatibility("https://x/playback", Some("nope")));
    }

    #[test]
    fn clocks_and_time_left_read_naturally() {
        assert_eq!(format_clock(0.), "0:00");
        assert_eq!(format_clock(65.), "1:05");
        assert_eq!(format_clock(3729.), "1:02:09");
        assert_eq!(format_clock(f64::NAN), "0:00");
        assert_eq!(time_left(10., 2050.), "34 min left");
        assert_eq!(time_left(0., 7500.), "2h 5m left");
        assert_eq!(time_left(100., 100.), "Finishing");
    }

    #[test]
    fn next_episode_crosses_seasons_and_skips_specials() {
        let media = Media {
            videos: vec![
                episode("s1e2", 1, 2),
                episode("s0e1", 0, 1),
                episode("s2e1", 2, 1),
                episode("s1e1", 1, 1),
            ],
            ..Default::default()
        };
        assert_eq!(next_episode(&media, "s1e1").unwrap().id, "s1e2");
        assert_eq!(next_episode(&media, "s1e2").unwrap().id, "s2e1");
        assert!(next_episode(&media, "s2e1").is_none());
        assert!(next_episode(&media, "missing").is_none());
    }

    fn source(addon: &str, name: &str, group: Option<&str>, playable: bool) -> StreamSource {
        let stream = serde_json::json!({
            "name": name,
            "url": if playable { "https://x/a.mkv" } else { "magnet:nope" },
            "behaviorHints": {"bingeGroup": group},
        });
        StreamSource {
            addon: addon.into(),
            stream: serde_json::from_value(stream).unwrap(),
        }
    }

    fn names(ranked: &[StreamSource]) -> Vec<String> {
        ranked.iter().map(|s| s.stream.label().to_owned()).collect()
    }

    #[test]
    fn sources_rank_by_what_played_last_and_keep_the_addons_order_otherwise() {
        let results = StreamResults {
            sources: vec![
                source("A", "4K", None, true),
                source("A", "dead", None, false),
                source("B", "720p", None, true),
                source("B", "1080p", Some("group"), true),
                source("C", "1080p", None, true),
            ],
            failures: vec![],
        };
        assert_eq!(
            names(&rank_sources(&results, None)),
            ["4K", "720p", "1080p", "1080p"]
        );
        let same_addon = LastSource {
            addon: "B".into(),
            name: Some("nope".into()),
            binge_group: None,
        };
        assert_eq!(rank_sources(&results, Some(&same_addon))[0].addon, "B");
        let same_label = LastSource {
            addon: "B".into(),
            name: Some("1080p".into()),
            binge_group: None,
        };
        assert_eq!(
            rank_sources(&results, Some(&same_label))[0].stream.label(),
            "1080p"
        );
        assert_eq!(rank_sources(&results, Some(&same_label))[0].addon, "B");
        let binge = LastSource {
            addon: "Z".into(),
            name: None,
            binge_group: Some("group".into()),
        };
        assert_eq!(rank_sources(&results, Some(&binge))[0].addon, "B");
        assert_eq!(
            LastSource::of(&rank_sources(&results, Some(&binge))[0])
                .binge_group
                .as_deref(),
            Some("group")
        );
    }

    fn progress(video: &str, seconds: f64) -> WatchProgress {
        WatchProgress {
            media: Media::default(),
            video_id: video.into(),
            episode_label: None,
            seconds,
            duration: 100.,
            updated_at: 1.,
        }
    }

    #[test]
    fn up_next_resumes_then_advances_then_starts_at_the_beginning() {
        let film = Media {
            id: "tt1".into(),
            kind: "movie".into(),
            ..Default::default()
        };
        assert_eq!(up_next(&film, None).as_deref(), Some("tt1"));

        let series = Media {
            kind: "series".into(),
            videos: vec![
                episode("special", 0, 1),
                episode("s1e1", 1, 1),
                episode("s1e2", 1, 2),
            ],
            ..Default::default()
        };
        assert_eq!(
            up_next(&series, None).as_deref(),
            Some("s1e1"),
            "specials aren't the start"
        );
        assert_eq!(
            up_next(&series, Some(&progress("s1e1", 50.))).as_deref(),
            Some("s1e1")
        );
        assert_eq!(
            up_next(&series, Some(&progress("s1e1", 99.))).as_deref(),
            Some("s1e2")
        );
        assert_eq!(
            up_next(&series, Some(&progress("s1e2", 99.))).as_deref(),
            Some("s1e2"),
            "last episode stays"
        );

        let unloaded = Media {
            kind: "series".into(),
            ..Default::default()
        };
        assert_eq!(
            up_next(&unloaded, Some(&progress("tt:1:4", 50.))).as_deref(),
            Some("tt:1:4")
        );
        assert_eq!(up_next(&unloaded, None), None, "needs metadata first");
    }

    #[test]
    fn episode_labels_use_titles_when_known() {
        let mut series = Media {
            kind: "series".into(),
            ..Default::default()
        };
        assert_eq!(episode_label(&series, "tt1:2:3").as_deref(), Some("S2 E3"));
        series.videos = vec![Episode {
            id: "tt1:2:3".into(),
            name: Some("Pilot".into()),
            season: Some(2),
            episode: Some(3),
            ..Default::default()
        }];
        assert_eq!(
            episode_label(&series, "tt1:2:3").as_deref(),
            Some("S2 E3  ·  Pilot")
        );
        assert_eq!(episode_label(&Media::default(), "tt1"), None);
    }

    #[test]
    fn seasons_come_from_episode_ids() {
        assert_eq!(season_of("tt123:2:3"), Some(2));
        assert_eq!(season_of("tt123"), None);
    }
}
