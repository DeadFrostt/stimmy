//! Video engines.
//!
//! macOS draws through a native view inserted directly beneath GPUI's in the
//! window, and GPUI's layer goes transparent during playback, so the controls
//! composite over real video with no pixel copying and engine features like
//! hardware decoding, HDR, and libass subtitles stay intact.
//!
//! Wayland has no subwindows, so Linux renders each frame into a buffer that
//! GPUI draws as an image instead. The controls still composite over real video,
//! but decoding is software-only: mpv's render API can't hand hardware surfaces
//! to a buffer.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub position: f64,
    pub duration: f64,
    pub buffered: f64,
    pub paused: bool,
    pub buffering: bool,
    pub ended: bool,
    /// A picture is on screen, so the loading stage can step aside.
    pub first_frame: bool,
    pub failed: Option<String>,
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
}

/// One decoded picture: BGRA, top row first, tightly packed.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

pub trait Engine {
    fn snapshot(&mut self) -> Snapshot;
    fn set_paused(&mut self, paused: bool);
    fn seek(&mut self, seconds: f64);
    fn set_volume(&mut self, volume: f32);
    fn set_speed(&mut self, speed: f32);
    fn select_audio(&mut self, id: &str);
    /// `None` turns the engine's own subtitles off.
    fn select_subtitle(&mut self, id: Option<&str>);
    fn shutdown(&mut self);
    fn name(&self) -> &'static str;

    /// The newest picture, if one is waiting. macOS engines draw natively, so
    /// they never produce one.
    fn take_frame(&mut self) -> Option<Frame> {
        None
    }

    /// Fires when a picture is ready to draw. macOS engines draw natively.
    fn frame_ready(&self) -> Option<async_channel::Receiver<()>> {
        None
    }

    /// The size the picture is drawn at, in device pixels, so an engine that
    /// renders into a buffer can scale to it.
    fn set_viewport(&mut self, _width: u32, _height: u32) {}
}
