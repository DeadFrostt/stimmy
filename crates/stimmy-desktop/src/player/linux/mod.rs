//! Video engines.
//!
//! Wayland has no subwindows, so there is no native view to place beneath
//! GPUI's the way macOS does. libmpv renders each frame into a buffer instead,
//! and the player draws it as an image with the controls composited over it.

mod mpv;

use super::Engine;

pub use mpv::available as mpv_available;

/// There is no native view to place on Linux; the player draws frames itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostView;

impl HostView {
    pub fn from_window(_window: &gpui_kit::Window) -> Option<Self> {
        Some(Self)
    }

    /// Wayland draws its own decorations, so there are no window buttons to hide.
    pub fn set_window_buttons_hidden(&self, _hidden: bool) {}
}

/// GPUI has no way to hide the pointer, so the controls simply fade.
pub fn hide_cursor_until_moved() {}

/// Keeps the display awake for the length of a video. Dropping it lets go.
///
/// Wayland has no portable way to ask for this without a D-Bus session, so the
/// compositor's own idle policy applies.
pub struct KeepAwake;

impl KeepAwake {
    pub fn begin() -> Self {
        Self
    }
}

/// Plays through libmpv, which is the only engine on Linux.
pub fn open(
    host: HostView,
    url: &str,
    filename: Option<&str>,
    start: f64,
    force_compatibility: bool,
) -> Result<Box<dyn Engine>, String> {
    let _ = (host, filename, force_compatibility);
    mpv::MpvEngine::open(url, start)
        .map(|engine| Box::new(engine) as Box<dyn Engine>)
        .map_err(|error| error.to_string())
}
