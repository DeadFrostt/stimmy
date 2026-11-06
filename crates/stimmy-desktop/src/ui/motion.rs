//! The app's motion vocabulary. The idea throughout is light: it arrives with a
//! title, travels around whatever is working, and settles. Everything here runs
//! for a bounded time or only while something is actually happening.

use super::theme::ink;
use gpui_kit::{prelude::*, *};
use std::time::Duration;

/// Decelerating arrival for content that has somewhere to be.
pub const ARRIVE_MS: u64 = 380;

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(4)
}

/// Arrives after `delay_ms`, rising a little as it fades up. Used in short
/// sequences, never on content that's already on screen.
pub fn rise(id: impl Into<SharedString>, delay_ms: u64, reduced: bool, el: Div) -> AnyElement {
    if reduced {
        return el.into_any_element();
    }
    let total = delay_ms + ARRIVE_MS;
    let start = delay_ms as f32 / total as f32;
    el.relative()
        .with_animation(
            ElementId::Name(id.into()),
            Animation::new(Duration::from_millis(total)),
            move |el, t| {
                let progress = ease_out(((t - start) / (1. - start)).clamp(0., 1.));
                el.opacity(progress).top(px((1. - progress) * 12.))
            },
        )
        .into_any_element()
}

/// The point at `position` (0 to 1, clockwise from the top-left corner's end) on a
/// rounded rectangle's edge.
pub fn perimeter_point(width: f32, height: f32, radius: f32, position: f32) -> (f32, f32) {
    let radius = radius.min(width / 2.).min(height / 2.);
    let (flat_x, flat_y) = (width - 2. * radius, height - 2. * radius);
    let arc = std::f32::consts::FRAC_PI_2 * radius;
    let perimeter = 2. * (flat_x + flat_y) + 4. * arc;
    let mut left = position.rem_euclid(1.) * perimeter;
    let corner = |cx: f32, cy: f32, start: f32, along: f32| {
        let angle = start + along / radius.max(0.001);
        (cx + radius * angle.cos(), cy + radius * angle.sin())
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    if left < flat_x {
        return (radius + left, 0.);
    }
    left -= flat_x;
    if left < arc {
        return corner(width - radius, radius, -FRAC_PI_2, left);
    }
    left -= arc;
    if left < flat_y {
        return (width, radius + left);
    }
    left -= flat_y;
    if left < arc {
        return corner(width - radius, height - radius, 0., left);
    }
    left -= arc;
    if left < flat_x {
        return (width - radius - left, height);
    }
    left -= flat_x;
    if left < arc {
        return corner(radius, height - radius, FRAC_PI_2, left);
    }
    left -= arc;
    if left < flat_y {
        return (0., height - radius - left);
    }
    left -= flat_y;
    corner(radius, radius, PI, left)
}

fn paint_beam(
    bounds: Bounds<Pixels>,
    radius: f32,
    colour: Hsla,
    head: f32,
    strength: f32,
    window: &mut Window,
) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if width < 2. || height < 2. || strength <= 0.001 {
        return;
    }
    let perimeter = 2. * (width + height);
    const TAIL: f32 = 0.3;
    let steps = ((perimeter * TAIL / 1.5) as usize).clamp(20, 260);
    let (origin_x, origin_y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let dot = |x: f32, y: f32, size: f32| {
        Bounds::new(
            point(px(origin_x + x - size / 2.), px(origin_y + y - size / 2.)),
            gpui_kit::size(px(size), px(size)),
        )
    };
    // Glow first, so the bright core sits on top of it.
    for index in (0..steps).step_by(5) {
        let along = index as f32 / steps as f32;
        let (x, y) = perimeter_point(width, height, radius, head - along * TAIL);
        let alpha = (1. - along).powi(2) * strength * 0.16;
        let glow = Hsla { a: alpha, ..colour };
        window.paint_quad(fill(dot(x, y, 10.), glow).corner_radii(px(5.)));
    }
    for index in 0..steps {
        let along = index as f32 / steps as f32;
        let (x, y) = perimeter_point(width, height, radius, head - along * TAIL);
        let alpha = (1. - along).powi(2) * strength;
        let core = Hsla { a: alpha, ..colour };
        window.paint_quad(fill(dot(x, y, 2.5), core).corner_radii(px(1.25)));
    }
}

/// A light that rides the edge of its parent, adapted from Beam's border beam for
/// GPUI, which has no shaders to run the original on. One lap acknowledges
/// something new; looping means work is under way, and stops when the work does.
/// Fills the parent, so the parent should be positioned and match the edge.
pub fn beam(
    id: impl Into<SharedString>,
    radius: f32,
    colour: Hsla,
    lap_ms: u64,
    looping: bool,
) -> AnyElement {
    let animation = Animation::new(Duration::from_millis(lap_ms));
    let animation = if looping {
        animation.repeat()
    } else {
        animation
    };
    div()
        .absolute()
        .inset_0()
        .with_animation(ElementId::Name(id.into()), animation, move |el, t| {
            // A single lap fades in and out, so it never starts or stops abruptly.
            let strength = if looping {
                1.
            } else {
                (t * std::f32::consts::PI).sin().powf(0.7)
            };
            let head = if looping {
                t
            } else {
                ease_out(t) * 0.92 + 0.04
            };
            el.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        paint_beam(bounds, radius, colour, head, strength, window)
                    },
                )
                .size_full(),
            )
        })
        .into_any_element()
}

/// A backdrop at full width and its own height. The width must be absolute: GPUI only
/// derives an image's height from an absolute width, and otherwise boxes it at the
/// file's own height, which sits large artwork lower down with a hard edge above it.
pub fn backdrop_image(path: std::path::PathBuf, width: f32) -> Img {
    img(path).w(px(width))
}

/// A title's accent, lifted so it reads as light on a dark room.
pub fn accent_light(rgb: [u8; 3]) -> Hsla {
    let colour: Hsla = gpui_kit::Rgba {
        r: rgb[0] as f32 / 255.,
        g: rgb[1] as f32 / 255.,
        b: rgb[2] as f32 / 255.,
        a: 1.,
    }
    .into();
    Hsla {
        s: colour.s.max(if colour.s > 0.12 { 0.72 } else { 0. }),
        l: colour.l.clamp(0.68, 0.82),
        ..colour
    }
}

/// White, for anything without a colour of its own yet.
pub fn no_accent() -> Hsla {
    ink(0.95)
}

#[cfg(test)]
mod tests {
    use super::{accent_light, perimeter_point};

    #[test]
    fn the_beam_stays_on_the_rounded_edge_all_the_way_round() {
        let (width, height, radius) = (150., 225., 8.);
        for step in 0..400 {
            let (x, y) = perimeter_point(width, height, radius, step as f32 / 400.);
            assert!(
                (-0.01..=width + 0.01).contains(&x) && (-0.01..=height + 0.01).contains(&y),
                "{x} {y}"
            );
            let on_straight = x.abs() < 0.01
                || (x - width).abs() < 0.01
                || y.abs() < 0.01
                || (y - height).abs() < 0.01;
            let dx = (radius - x).max(x - (width - radius)).max(0.);
            let dy = (radius - y).max(y - (height - radius)).max(0.);
            let on_corner =
                dx > 0. && dy > 0. && ((dx * dx + dy * dy).sqrt() - radius).abs() < 0.05;
            assert!(on_straight || on_corner, "{x} {y} left the edge");
        }
    }

    #[test]
    fn the_beam_is_continuous_and_wraps_cleanly() {
        let mut previous = perimeter_point(100., 40., 20., 0.);
        for step in 1..=1000 {
            let next = perimeter_point(100., 40., 20., step as f32 / 1000.);
            let jump = ((next.0 - previous.0).powi(2) + (next.1 - previous.1).powi(2)).sqrt();
            assert!(jump < 1., "jumped {jump} at {step}");
            previous = next;
        }
    }

    #[test]
    fn accents_read_as_light_and_neutrals_stay_neutral() {
        let red = accent_light([120, 20, 20]);
        assert!(red.l >= 0.68 && red.s >= 0.7);
        assert_eq!(accent_light([128, 128, 128]).s, 0.);
    }
}
