//! SRT and WebVTT cues, and fetching them from add-ons.

use crate::{
    client::StremioClient,
    error::{Error, Result},
    model::Addon,
    protocol::resource_url,
};
use serde::Deserialize;
use std::{io::Read, thread};

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SubtitleTrack {
    #[serde(default)]
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub lang: String,
}

impl SubtitleTrack {
    pub fn label(&self) -> String {
        if self.lang.is_empty() {
            "Unknown".into()
        } else {
            language_name(&self.lang)
        }
    }
}

/// English names for the codes add-ons commonly send, or the code itself.
pub fn language_name(code: &str) -> String {
    let name = match code.to_ascii_lowercase().as_str() {
        "en" | "eng" => "English",
        "es" | "spa" => "Spanish",
        "fr" | "fre" | "fra" => "French",
        "de" | "ger" | "deu" => "German",
        "it" | "ita" => "Italian",
        "pt" | "por" => "Portuguese",
        "pob" | "pt-br" => "Portuguese (Brazil)",
        "ru" | "rus" => "Russian",
        "ja" | "jpn" => "Japanese",
        "ko" | "kor" => "Korean",
        "zh" | "chi" | "zho" => "Chinese",
        "ar" | "ara" => "Arabic",
        "hi" | "hin" => "Hindi",
        "nl" | "dut" | "nld" => "Dutch",
        "sv" | "swe" => "Swedish",
        "pl" | "pol" => "Polish",
        "tr" | "tur" => "Turkish",
        _ => return code.to_owned(),
    };
    name.to_owned()
}

fn timestamp(text: &str) -> Option<f64> {
    let cleaned = text.trim().replace(',', ".");
    let parts: Vec<&str> = cleaned.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total = 0.;
    for part in parts {
        let value: f64 = part.parse().ok()?;
        if value < 0. {
            return None;
        }
        total = total * 60. + value;
    }
    Some(total)
}

fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

pub fn parse(input: &str) -> Vec<Cue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues: Vec<Cue> = normalized
        .split("\n\n")
        .filter_map(|block| {
            let lines: Vec<&str> = block.lines().collect();
            let index = lines.iter().position(|line| line.contains(" --> "))?;
            let (start, rest) = lines[index].split_once(" --> ")?;
            let start = timestamp(start)?;
            let end = timestamp(rest.split_whitespace().next()?)?;
            let text = strip_markup(&lines[index + 1..].join("\n"))
                .trim()
                .to_owned();
            (end > start && !text.is_empty()).then_some(Cue { start, end, text })
        })
        .collect();
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    cues
}

/// The cue showing at a time, by binary search since this runs every tick.
pub fn cue_at(cues: &[Cue], seconds: f64) -> Option<&Cue> {
    let after = cues.partition_point(|cue| cue.start <= seconds);
    cues[..after]
        .iter()
        .rev()
        .take(4)
        .find(|cue| cue.end > seconds)
}

#[derive(Deserialize)]
struct SubtitleResponse {
    #[serde(default)]
    subtitles: Vec<serde_json::Value>,
}

impl StremioClient {
    pub fn subtitles(&self, kind: &str, video_id: &str, addons: &[Addon]) -> Vec<SubtitleTrack> {
        let targets: Vec<&Addon> = addons
            .iter()
            .filter(|addon| addon.manifest.supports("subtitles", kind, video_id))
            .collect();
        let mut seen = std::collections::HashSet::new();
        thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|addon| {
                    scope.spawn(move || -> Result<Vec<SubtitleTrack>> {
                        let url =
                            resource_url(&addon.transport_url, "subtitles", kind, video_id, &[])?;
                        let response: SubtitleResponse = self.get_json(url)?;
                        Ok(response
                            .subtitles
                            .into_iter()
                            .filter_map(|value| serde_json::from_value(value).ok())
                            .collect())
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().ok().and_then(Result::ok).unwrap_or_default())
                .filter(|track| seen.insert(track.url.clone()))
                .collect()
        })
    }

    pub fn subtitle_cues(&self, url: &str) -> Result<Vec<Cue>> {
        const MAX: u64 = 8 * 1024 * 1024;
        let parsed = url::Url::parse(url).map_err(|_| Error::Unreadable)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(Error::Unreadable);
        }
        let response = self.http.get(parsed).send()?.error_for_status()?;
        let mut bytes = Vec::new();
        response
            .take(MAX)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Offline)?;
        let cues = parse(&String::from_utf8_lossy(&bytes));
        if cues.is_empty() {
            Err(Error::Unreadable)
        } else {
            Ok(cues)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_and_webvtt_parse_with_markup_removed() {
        let srt = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i> &amp; welcome\r\nSecond line\r\n\r\n2\r\n00:00:03,000 --> 00:00:02,000\r\nBackwards\r\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, 1.);
        assert_eq!(cues[0].end, 2.5);
        assert_eq!(cues[0].text, "Hello & welcome\nSecond line");
        let vtt = "WEBVTT\n\n00:05.000 --> 00:06.000 align:center\nShort time\n";
        assert_eq!(parse(vtt)[0].start, 5.);
    }

    #[test]
    fn cue_lookup_finds_the_showing_cue_only() {
        let cues = parse("00:01.000 --> 00:03.000\nA\n\n00:05.000 --> 00:06.000\nB\n");
        assert_eq!(cue_at(&cues, 0.5), None);
        assert_eq!(cue_at(&cues, 2.).unwrap().text, "A");
        assert_eq!(cue_at(&cues, 4.), None);
        assert_eq!(cue_at(&cues, 5.5).unwrap().text, "B");
    }

    #[test]
    fn languages_get_readable_names() {
        assert_eq!(language_name("eng"), "English");
        assert_eq!(language_name("xyz"), "xyz");
    }
}
