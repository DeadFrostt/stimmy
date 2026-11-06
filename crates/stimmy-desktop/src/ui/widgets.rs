//! Buttons, posters, and small shared pieces.

use super::theme::*;
use crate::app::{Art, Stimmy};
use gpui_kit::{
    component::{Icon, IconName},
    prelude::*,
    *,
};
use std::time::Duration;
use stimmy_core::Media;

pub const POSTER_GAP: f32 = 16.;

pub fn grid_columns(width: f32) -> usize {
    ((width + POSTER_GAP) / (168. + POSTER_GAP))
        .floor()
        .clamp(3., 9.) as usize
}

/// Every control shares these, so rows line up wherever they meet.
pub const CONTROL_HEIGHT: f32 = 32.;
pub const CHIP_HEIGHT: f32 = 28.;

fn control(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .tab_index(0)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(CONTROL_HEIGHT))
        .rounded_full()
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .border_1()
        .border_color(transparent_black())
        .focus_visible(|style| style.border_color(ink(0.9)))
        .active(|style| style.opacity(0.72))
        // Keeps a click on a control from also landing on whatever it sits on,
        // such as an episode row or the video.
        .occlude()
}

fn solid(el: Stateful<Div>) -> Stateful<Div> {
    el.bg(ink(0.94))
        .text_color(rgb(CANVAS))
        .hover(|style| style.bg(ink(1.)))
}

fn quiet(el: Stateful<Div>) -> Stateful<Div> {
    el.bg(ink(0.11))
        .text_color(ink(0.95))
        .hover(|style| style.bg(ink(0.18)))
}

/// The one strong action on a page.
pub fn primary_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    solid(control(id, label))
        .px(px(15.))
        .child(label.to_owned())
}

pub fn quiet_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    quiet(control(id, label))
        .px(px(14.))
        .child(label.to_owned())
}

/// A smaller quiet button, for choices shown in a group.
pub fn chip(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    quiet(control(id, label))
        .h(px(CHIP_HEIGHT))
        .px(px(12.))
        .child(label.to_owned())
}

/// A button that can't be pressed right now, and says why through its label.
pub fn busy_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_HEIGHT))
        .px(px(14.))
        .rounded_full()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .bg(ink(0.07))
        .text_color(tertiary())
        .child(label.to_owned())
}

pub fn icon_label_button(id: impl Into<ElementId>, icon: IconName, label: &str) -> Stateful<Div> {
    quiet(control(id, label))
        .pl(px(10.))
        .pr(px(14.))
        .child(Icon::new(icon).size(px(15.)))
        .child(label.to_owned())
}

/// `icon_label_button` for a page's strongest action.
pub fn primary_icon_button(id: impl Into<ElementId>, icon: IconName, label: &str) -> Stateful<Div> {
    solid(control(id, label))
        .pl(px(10.))
        .pr(px(15.))
        .child(Icon::new(icon).size(px(15.)))
        .child(label.to_owned())
}

/// Save and In Library. The check pops as it arrives, to acknowledge the save.
pub fn save_button(
    id: impl Into<ElementId>,
    saved: bool,
    label_saved: &str,
    label_unsaved: &str,
    reduced: bool,
) -> Stateful<Div> {
    let label = if saved { label_saved } else { label_unsaved };
    let icon = Icon::new(if saved {
        IconName::Check
    } else {
        IconName::Plus
    });
    let id: ElementId = id.into();
    let icon_box = div().size(px(15.)).flex().items_center().justify_center();
    let icon_box = if reduced || !saved {
        icon_box.child(icon.size(px(15.)).into_any_element())
    } else {
        icon_box.child(
            icon.with_spring(
                "save-pop",
                SpringAnimation::new(SpringConfig::new(520., 18., 1.))
                    .to(15.)
                    .from(5.),
                |el, value: f32| el.size(px(value)),
            )
            .into_any_element(),
        )
    };
    quiet(control(id, label))
        .pl(px(10.))
        .pr(px(14.))
        .child(icon_box)
        .child(label.to_owned())
}

/// `icon_button` with one of Stimmy's own icons.
pub fn svg_button(id: impl Into<ElementId>, path: &'static str, label: &str) -> Stateful<Div> {
    control(id, label)
        .w(px(CONTROL_HEIGHT))
        .text_color(ink(0.86))
        .hover(|style| style.bg(ink(0.12)))
        .child(svg().path(path).size(px(16.)).text_color(ink(0.9)))
}

/// `icon_label_button` with one of Stimmy's own icons.
pub fn svg_label_button(
    id: impl Into<ElementId>,
    path: &'static str,
    label: &str,
) -> Stateful<Div> {
    quiet(control(id, label))
        .pl(px(10.))
        .pr(px(14.))
        .child(svg().path(path).size(px(15.)).text_color(ink(0.95)))
        .child(label.to_owned())
}

pub fn human_size(bytes: u64) -> String {
    match bytes {
        0..1_000_000 => format!("{} KB", bytes / 1000),
        1_000_000..1_000_000_000 => format!("{} MB", bytes / 1_000_000),
        _ => format!("{:.1} GB", bytes as f64 / 1e9),
    }
}

/// A plain icon, for controls that sit on open space.
pub fn icon_button(id: impl Into<ElementId>, icon: IconName, label: &str) -> Stateful<Div> {
    control(id, label)
        .w(px(CONTROL_HEIGHT))
        .text_color(ink(0.86))
        .hover(|style| style.bg(ink(0.12)))
        .child(Icon::new(icon).size(px(16.)))
}

/// A filled icon, for controls that float over moving content.
pub fn floating_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &str,
) -> Stateful<Div> {
    quiet(control(id, label))
        .w(px(CONTROL_HEIGHT))
        .bg(hsla(0., 0., 0., 0.42))
        .child(Icon::new(icon).size(px(16.)))
}

/// A text field in the shared pill shape, the same height as the buttons beside it.
pub fn field(input: &Entity<component::input::InputState>, label: &str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(CONTROL_HEIGHT))
        .px(px(4.))
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(ink(0.16))
        .bg(ink(0.05))
        .child(
            component::input::Input::new(input)
                .aria_label(label.to_owned())
                .appearance(false)
                .bordered(false)
                .focus_bordered(false),
        )
}

pub fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .line_height(relative(1.5))
        .text_color(secondary())
        .child(text.into())
}

pub fn error_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .line_height(relative(1.5))
        .text_color(error_tint())
        .child(text.into())
}

/// A section heading, exposed as one to assistive technology.
pub fn heading(id: impl Into<SharedString>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(ElementId::Name(id.into()))
        .role(Role::Heading)
        .aria_level(2)
        .aria_label(text.clone())
        .text_size(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

pub fn meta_line(media: &Media) -> String {
    let genres = media
        .genres
        .iter()
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    [
        Some(media.year().to_owned()),
        media.runtime.clone(),
        (!genres.is_empty()).then_some(genres),
        media
            .imdb_rating
            .as_ref()
            .map(|rating| format!("IMDb {rating}")),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("  ·  ")
}

impl Stimmy {
    /// Fresh art fades up briefly instead of popping in.
    pub fn art_image(&self, path: std::path::PathBuf, key: String, cx: &App) -> AnyElement {
        let image = img(path).size_full().object_fit(ObjectFit::Cover);
        if cx.reduce_motion() {
            image.into_any_element()
        } else {
            image
                .with_animation(
                    SharedString::from(format!("art-{key}")),
                    Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t),
                )
                .into_any_element()
        }
    }

    pub fn poster(
        &self,
        media: &Media,
        width: f32,
        focused: bool,
        with_meta: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = media.key();
        let hovered = self.hovered_poster.as_deref() == Some(key.as_str());
        let lifted = hovered || focused;
        let height = (width * 1.5).round();
        let art = self.art_path(Art::Original, media.poster.as_ref());
        let art_gone = self.art_gone(Art::Original, media.poster.as_ref());
        let (hover_media, open_media) = (media.clone(), media.clone());
        let label = format!(
            "{}, {}{}",
            media.name,
            media.kind_label(),
            if media.year().is_empty() {
                String::new()
            } else {
                format!(", {}", media.year())
            }
        );

        let reduced = cx.reduce_motion();
        let accent = self.accent_for(media);
        let art_layer = div()
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(ink(0.06))
            .when_some(art, |el, path| {
                el.child(self.art_image(path, key.clone(), cx))
            })
            // A title with no artwork still says what it is.
            .when(art_gone, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(12.))
                        .text_size(px(13.))
                        .text_center()
                        .text_color(secondary())
                        .child(media.name.clone()),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        ink(0.95)
                    } else if hovered && reduced {
                        accent
                    } else {
                        ink(0.07)
                    }),
            );
        // Grows from its centre into its own margin, so nothing around it moves.
        let grown = div()
            .absolute()
            .when(lifted, |el| {
                el.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.5),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(26.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .rounded(px(8.))
            .child(art_layer)
            .when(hovered && !reduced, |el| {
                el.child(super::motion::beam(
                    format!("poster-beam-{key}"),
                    8.,
                    accent,
                    1100,
                    false,
                ))
            });
        let growth = if lifted { (width * 0.022).round() } else { 0. };
        let grown: AnyElement = if reduced {
            grown
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .into_any_element()
        } else {
            grown
                .with_spring(
                    SharedString::from(format!("grow-{key}")),
                    SpringAnimation::new(spring_soft()).to(growth),
                    |el, value: f32| {
                        el.top(px(-value))
                            .left(px(-value))
                            .right(px(-value))
                            .bottom(px(-value))
                    },
                )
                .into_any_element()
        };
        let frame = div().relative().w(px(width)).h(px(height)).child(grown);

        div()
            .id(SharedString::from(format!("poster-{key}")))
            .role(Role::Link)
            .aria_label(label)
            .tab_index(0)
            .rounded(px(8.))
            .focus_visible(|style| style.bg(ink(0.14)))
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(9.))
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.poster_hovered(&hover_media, *hovered, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.open_title(open_media.clone(), cx);
            }))
            .child(frame)
            .child(
                div()
                    .w(px(width))
                    .min_w_0()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if lifted { ink(1.) } else { ink(0.86) })
                    .truncate()
                    .child(media.name.clone()),
            )
            .when(with_meta, |el| {
                let year = media.year();
                let text = if year.is_empty() {
                    media.kind_label().to_owned()
                } else {
                    format!("{year}  ·  {}", media.kind_label())
                };
                el.child(
                    div()
                        .mt(px(-6.))
                        .text_size(px(12.))
                        .text_color(tertiary())
                        .child(text),
                )
            })
            .into_any_element()
    }

    pub fn poster_grid(&self, items: &[Media], width: f32, cx: &mut Context<Self>) -> Div {
        let columns = grid_columns(width);
        let item_width = ((width - POSTER_GAP * (columns - 1) as f32) / columns as f32).floor();
        let cursor = self.cursor.map(|c| c.col);
        div()
            .flex()
            .flex_wrap()
            .gap_x(px(POSTER_GAP))
            .gap_y(px(26.))
            .children(items.iter().enumerate().map(|(index, media)| {
                // A list arriving as a list, with the wait capped so it never lags.
                let poster = self.poster(media, item_width, cursor == Some(index), true, cx);
                super::motion::rise(
                    format!("cascade-{}", media.key()),
                    index.min(10) as u64 * 26,
                    cx.reduce_motion(),
                    div().child(poster),
                )
            }))
    }

    pub fn notice_banner(&self, notice: String, gutter: f32, cx: &mut Context<Self>) -> Div {
        div()
            .pt(px(TOP_BAR + 8.))
            .px(px(gutter))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(div().flex_1().min_w_0().child(error_text(notice)))
            .child(
                quiet_button("dismiss-notice", "Dismiss").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    },
                )),
            )
    }
}
