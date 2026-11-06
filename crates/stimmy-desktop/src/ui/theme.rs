//! Colour, size, and motion values. See DESIGN.md.

use gpui_kit::*;

/// Neutral black, deliberately with no hue: all colour here is the artwork's.
pub const CANVAS: u32 = 0x0a0a0b;
pub const TOP_BAR: f32 = 54.;

/// White at some strength: the only colour the interface itself uses.
pub fn ink(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}
pub const INK_SECONDARY: f32 = 0.74;
pub const INK_TERTIARY: f32 = 0.6;

pub fn secondary() -> Hsla {
    ink(INK_SECONDARY)
}
pub fn tertiary() -> Hsla {
    ink(INK_TERTIARY)
}
pub fn hairline() -> Hsla {
    ink(0.13)
}
pub fn error_tint() -> Hsla {
    hsla(10. / 360., 0.9, 0.8, 1.)
}

/// The edge moving toward its destination arrives first, and the other follows,
/// so the indicator stretches between tabs and settles on the new one.
pub fn spring_lead() -> SpringConfig {
    SpringConfig::new(560., 40., 1.)
}
pub fn spring_trail() -> SpringConfig {
    SpringConfig::new(280., 30., 1.)
}
pub fn spring_soft() -> SpringConfig {
    SpringConfig::new(320., 32., 1.)
}
