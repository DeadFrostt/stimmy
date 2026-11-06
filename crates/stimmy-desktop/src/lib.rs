//! The Stimmy desktop app: application state, the interface, and video playback.

pub mod app;
pub mod player;
pub mod ui;

use app::actions::*;
use gpui_kit::{App, AssetSource, KeyBinding, SharedString};
use std::borrow::Cow;

/// Kit's icon set, plus the few the player needs that it doesn't include.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let own: Option<&'static str> = match path {
            "stimmy/volume.svg" => Some(include_str!("../assets/volume.svg")),
            "stimmy/mute.svg" => Some(include_str!("../assets/mute.svg")),
            "stimmy/captions.svg" => Some(include_str!("../assets/captions.svg")),
            "stimmy/audio.svg" => Some(include_str!("../assets/audio.svg")),
            "stimmy/download.svg" => Some(include_str!("../assets/download.svg")),
            "stimmy/source.svg" => Some(include_str!("../assets/source.svg")),
            _ => None,
        };
        match own {
            Some(svg) => Ok(Some(Cow::Borrowed(svg.as_bytes()))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }
    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

/// Dark, with a transparent root. Kit's root paints the theme's background over
/// the whole window, which would hide video under GPUI's layer. Every Stimmy page
/// paints its own canvas, so nothing relies on it.
pub fn apply_theme(cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::global_mut(cx).tokens.background = gpui_kit::transparent_black().into();
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", OpenHome, Some("Stimmy")),
        KeyBinding::new("cmd-2", OpenSearch, Some("Stimmy")),
        KeyBinding::new("cmd-f", OpenSearch, Some("Stimmy")),
        KeyBinding::new("cmd-3", OpenLibrary, Some("Stimmy")),
        KeyBinding::new("cmd-4", OpenDownloads, Some("Stimmy")),
        KeyBinding::new("cmd-5", OpenSettings, Some("Stimmy")),
        KeyBinding::new("cmd-,", OpenSettings, Some("Stimmy")),
        KeyBinding::new("cmd-r", Refresh, Some("Stimmy")),
        KeyBinding::new("cmd-d", ToggleSave, Some("Stimmy")),
        KeyBinding::new("escape", Back, Some("Stimmy")),
        KeyBinding::new("[", PreviousHero, Some("Stimmy")),
        KeyBinding::new("]", NextHero, Some("Stimmy")),
        KeyBinding::new("tab", FocusNext, Some("Stimmy")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("Stimmy")),
    ]);
}
