//! Video engines.
//!
//! Neither draws through GPUI. Each owns a native view inserted directly beneath
//! GPUI's in the window, and GPUI's layer goes transparent during playback, so
//! the controls composite over real video with no pixel copying and engine
//! features like hardware decoding, HDR, and libass subtitles stay intact.

mod av;
mod mpv;

use objc2::{MainThreadMarker, msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSProcessInfo, NSString};

pub use mpv::available as mpv_available;

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
}

/// GPUI's root NSView, for placing a video view beneath it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostView(pub *mut AnyObject);

impl HostView {
    /// Adds `view`, sized to fill the window, directly behind GPUI's view.
    pub(crate) fn insert_below(&self, view: *mut AnyObject) {
        const NS_WINDOW_BELOW: isize = -1;
        const WIDTH_AND_HEIGHT_SIZABLE: usize = 2 | 16;
        unsafe {
            let parent: *mut AnyObject = msg_send![self.0, superview];
            if parent.is_null() {
                return;
            }
            let frame: objc2_core_foundation::CGRect = msg_send![parent, bounds];
            let _: () = msg_send![view, setFrame: frame];
            let _: () = msg_send![view, setAutoresizingMask: WIDTH_AND_HEIGHT_SIZABLE];
            let _: () = msg_send![parent, addSubview: view, positioned: NS_WINDOW_BELOW, relativeTo: self.0];
        }
    }

    pub(crate) fn backing_scale(&self) -> f64 {
        unsafe {
            let window: *mut AnyObject = msg_send![self.0, window];
            if window.is_null() {
                return 2.;
            }
            msg_send![window, backingScaleFactor]
        }
    }

    /// Hides the traffic lights along with the rest of the controls.
    pub fn set_window_buttons_hidden(&self, hidden: bool) {
        unsafe {
            let window: *mut AnyObject = msg_send![self.0, window];
            if window.is_null() {
                return;
            }
            for kind in 0isize..3 {
                let button: *mut AnyObject = msg_send![window, standardWindowButton: kind];
                if !button.is_null() {
                    let _: () = msg_send![button, setHidden: hidden];
                }
            }
        }
    }
}

pub fn hide_cursor_until_moved() {
    unsafe {
        let cursor = objc2::class!(NSCursor);
        let _: () = msg_send![cursor, setHiddenUntilMouseMoves: true];
    }
}

/// Keeps the display awake for the length of a video. Dropping it lets go.
pub struct KeepAwake(Option<Retained<AnyObject>>);

impl KeepAwake {
    pub fn begin() -> Self {
        // NSActivityUserInitiated | NSActivityIdleDisplaySleepDisabled
        const OPTIONS: u64 = 0x00FF_FFFF | (1 << 40);
        let info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("Playing video");
        let token: Option<Retained<AnyObject>> =
            unsafe { msg_send![&info, beginActivityWithOptions: OPTIONS, reason: &*reason] };
        Self(token)
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            let info = NSProcessInfo::processInfo();
            let _: () = unsafe { msg_send![&info, endActivity: &*token] };
        }
    }
}

/// Plays with AVFoundation where it can, and mpv for what it can't open.
pub fn open(
    host: HostView,
    url: &str,
    filename: Option<&str>,
    start: f64,
    force_compatibility: bool,
) -> Result<Box<dyn Engine>, String> {
    assert!(
        MainThreadMarker::new().is_some(),
        "engines live on the main thread"
    );
    let compatibility =
        force_compatibility || stimmy_core::playback::prefers_compatibility(url, filename);
    if !compatibility {
        return av::AvEngine::open(host, url, start)
            .map(|engine| Box::new(engine) as Box<dyn Engine>);
    }
    mpv::MpvEngine::open(host, url, start)
        .map(|engine| Box::new(engine) as Box<dyn Engine>)
        .map_err(|error| error.to_string())
}
