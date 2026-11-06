//! Home: one title on a full-bleed stage, then shelves.

use super::{
    motion::{backdrop_image, beam, rise},
    theme::*,
    widgets::*,
};
use crate::app::{Art, Cursor, Stimmy};
use gpui_kit::{component::IconName, prelude::*, *};
use std::time::Duration;
use stimmy_core::Media;

impl Stimmy {
    /// The stage image. It sits on a matte, so it dissolves into the room's light.
    /// A new one settles from slightly larger while the last fades away beneath it,
    /// and `parallax` lets it fall behind the page as you scroll.
    pub fn stage_backdrop(
        &self,
        media: &Media,
        previous: Option<&Media>,
        viewport: f32,
        parallax: f32,
        cx: &App,
    ) -> Div {
        let reduced = cx.reduce_motion();
        let matte = self.art_path(Art::Matte, media.background.as_ref());
        let outgoing = previous
            .filter(|old| old.key() != media.key() && !reduced)
            .and_then(|old| self.art_path(Art::Matte, old.background.as_ref()))
            .map(|path| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(viewport))
                    .child(backdrop_image(path, viewport))
                    .with_animation(
                        SharedString::from(format!("matte-out-{}", media.key())),
                        Animation::new(Duration::from_millis(700)).with_easing(ease_out_quint()),
                        |el, t| el.opacity(1. - t),
                    )
            });
        div()
            .absolute()
            .top(px(parallax))
            .left_0()
            .w(px(viewport))
            .children(outgoing)
            .when_some(matte, |el, path| {
                // Full width at its own height: cropping would cut off the matte's
                // fade and leave a hard edge across the page.
                let image = backdrop_image(path, viewport).absolute().top_0().left_0();
                if reduced {
                    el.child(image.into_any_element())
                } else {
                    el.child(
                        image
                            .with_animation(
                                SharedString::from(format!("matte-{}", media.key())),
                                Animation::new(Duration::from_millis(1100))
                                    .with_easing(ease_out_quint()),
                                move |el, t| {
                                    el.opacity((t * 3.).min(1.))
                                        .w(px(viewport * (1.045 - 0.045 * t)))
                                        .left(px(-viewport * 0.0225 * (1. - t)))
                                },
                            )
                            .into_any_element(),
                    )
                }
            })
    }

    /// The studio's title treatment when the catalog has one, or plain type.
    pub fn title_mark(&self, media: &Media, large: bool) -> AnyElement {
        let logo = self.art_path(Art::Logo, media.logo.as_ref());
        let box_height = if large { 76. } else { 60. };
        if let Some(path) = logo {
            div()
                .id(SharedString::from(format!("mark-{}", media.key())))
                .role(Role::Heading)
                .aria_level(1)
                .aria_label(media.name.clone())
                .child(
                    img(path)
                        .h(px(box_height))
                        .max_w(px(if large { 380. } else { 320. }))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        } else {
            let waiting = !self.art_gone(Art::Logo, media.logo.as_ref());
            div()
                .id(SharedString::from(format!("mark-{}", media.key())))
                .role(Role::Heading)
                .aria_level(1)
                .aria_label(media.name.clone())
                .max_w(px(620.))
                .text_size(px(if large { 44. } else { 38. }))
                .line_height(relative(1.06))
                .font_weight(FontWeight::BOLD)
                .when(waiting, |el| el.opacity(0.))
                .child(media.name.clone())
                .into_any_element()
        }
    }

    fn stage(&self, viewport: f32, height: f32, gutter: f32, cx: &mut Context<Self>) -> AnyElement {
        let titles: Vec<Media> = self.stage_titles().into_iter().cloned().collect();
        let stage_height = (height * 0.74).clamp(440., 720.);
        let Some(media) = titles.get(self.hero).cloned() else {
            // Nothing to show yet: hold the stage's place rather than jumping.
            let failed = self.shelves.iter().find_map(|s| s.error.clone());
            return div()
                .id("stage")
                .h(px(stage_height))
                .flex()
                .flex_col()
                .justify_end()
                .gap(px(14.))
                .px(px(gutter))
                .pb(px(28.))
                .when_some(failed, |el, error| {
                    el.child(error_text(error)).child(
                        div().flex().child(
                            primary_button("retry", "Try again")
                                .on_click(cx.listener(|this, _, _, cx| this.load_shelves(cx))),
                        ),
                    )
                })
                .into_any_element();
        };
        let (play, open, save) = (media.clone(), media.clone(), media.clone());
        let saved = self.saved(&media);
        let meta = {
            let line = meta_line(&media);
            if line.is_empty() {
                media.kind_label().to_owned()
            } else {
                line
            }
        };
        let hero = self.hero;
        let count = titles.len();
        let reduced = cx.reduce_motion();
        let key = media.key();
        let accent = self.accent_for(&media);
        // The stage recedes as the page moves over it.
        let scrolled = (-f32::from(self.main_scroll.offset().y)).max(0.);
        let copy_opacity = if reduced {
            1.
        } else {
            1. - (scrolled / (stage_height * 0.7)).clamp(0., 1.)
        };
        div()
            .id("stage")
            .role(Role::Group)
            .aria_label("Featured")
            .relative()
            .h(px(stage_height))
            .w_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(self.stage_backdrop(
                &media,
                self.previous_hero.as_ref(),
                viewport,
                if reduced { 0. } else { scrolled * 0.38 },
                cx,
            ))
            .child(
                div()
                    .id(SharedString::from(format!("stage-copy-{}", media.key())))
                    .relative()
                    .opacity(copy_opacity)
                    .px(px(gutter))
                    .pb(px(26.))
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(32.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .max_w(px(540.))
                            .child(rise(
                                format!("stage-mark-{key}"),
                                0,
                                reduced,
                                div().child(self.title_mark(&media, true)),
                            ))
                            .child(rise(
                                format!("stage-meta-{key}"),
                                70,
                                reduced,
                                div().text_size(px(13.)).text_color(ink(0.82)).child(meta),
                            ))
                            .when_some(media.description.clone(), |el, text| {
                                el.child(rise(
                                    format!("stage-text-{key}"),
                                    130,
                                    reduced,
                                    div()
                                        .text_size(px(14.))
                                        .line_height(relative(1.5))
                                        .text_color(ink(0.8))
                                        .line_clamp(3)
                                        .child(text),
                                ))
                            })
                            .child(rise(
                                format!("stage-actions-{key}"),
                                190,
                                reduced,
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .mt(px(6.))
                                    .child(
                                        // One lap of the title's light says it's arrived.
                                        div()
                                            .relative()
                                            .child(
                                                primary_icon_button(
                                                    "stage-play",
                                                    IconName::Play,
                                                    "Play",
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.play_title(play.clone(), cx)
                                                })),
                                            )
                                            .when(!reduced, |el| {
                                                el.child(
                                                    div()
                                                        .absolute()
                                                        .top(px(-2.))
                                                        .left(px(-2.))
                                                        .right(px(-2.))
                                                        .bottom(px(-2.))
                                                        .child(beam(
                                                            format!("stage-beam-{key}"),
                                                            18.,
                                                            accent,
                                                            1300,
                                                            false,
                                                        )),
                                                )
                                            }),
                                    )
                                    .child(quiet_button("stage-open", "Details").on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.open_title(open.clone(), cx)
                                        }),
                                    ))
                                    .child(
                                        save_button(
                                            "stage-save",
                                            saved,
                                            "In Library",
                                            "Save",
                                            reduced,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.toggle_save(save.clone(), cx)
                                            }),
                                        ),
                                    ),
                            )),
                    )
                    .when(count > 1, |el| {
                        el.child(div().flex().items_center().gap(px(2.)).pb(px(8.)).children(
                            titles.iter().enumerate().map(|(index, title)| {
                                div()
                                    .id(SharedString::from(format!("stage-pick-{index}")))
                                    .role(Role::Button)
                                    .aria_label(format!("Feature {}", title.name))
                                    .aria_selected(index == hero)
                                    .px(px(3.))
                                    .py(px(8.))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.show_hero(index, cx)
                                        }),
                                    )
                                    .child(
                                        div()
                                            .w(px(if index == hero { 28. } else { 18. }))
                                            .h(px(2.))
                                            .rounded_full()
                                            .bg(ink(if index == hero { 0.95 } else { 0.32 }))
                                            .hover(|style| style.bg(ink(0.7))),
                                    )
                            }),
                        ))
                    }),
            )
            .into_any_element()
    }

    fn shelf(
        &self,
        row: usize,
        viewport: f32,
        gutter: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = &self.shelves[row];
        if state.items.is_empty() && !state.loading {
            return None;
        }
        let width = if viewport > 1600. {
            184.
        } else if viewport < 900. {
            132.
        } else {
            156.
        };
        let handle = state.scroll.clone();
        let (back, forward) = (handle.clone(), handle.clone());
        let page = (viewport - gutter * 2.) * 0.8;
        let cursor = self.cursor.filter(|c: &Cursor| c.row == row).map(|c| c.col);
        let items = state.items.clone();
        let placeholders = items.is_empty();
        Some(
            div()
                .id(SharedString::from(format!("shelf-{row}")))
                .role(Role::Group)
                .aria_label(state.shelf.title.clone())
                .relative()
                .flex()
                .flex_col()
                .gap(px(14.))
                .pb(px(30.))
                .child(
                    div()
                        .px(px(gutter))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(heading(
                            format!("shelf-title-{row}"),
                            state.shelf.title.clone(),
                        ))
                        .child(
                            div()
                                .flex()
                                .gap(px(2.))
                                .child(
                                    icon_button(
                                        format!("shelf-back-{row}"),
                                        IconName::ChevronLeft,
                                        "Scroll back",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| this.scroll_shelf(&back, -page, cx),
                                    )),
                                )
                                .child(
                                    icon_button(
                                        format!("shelf-next-{row}"),
                                        IconName::ChevronRight,
                                        "Scroll on",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| this.scroll_shelf(&forward, page, cx),
                                    )),
                                ),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("rail-{row}")))
                        .flex()
                        .gap(px(POSTER_GAP))
                        .pt(px(6.))
                        .px(px(gutter))
                        .overflow_x_scroll()
                        .restrict_scroll_to_axis()
                        .track_scroll(&handle)
                        .when(placeholders, |el| {
                            // Static placeholders: loading here is brief and shouldn't shimmer.
                            el.children((0..10).map(|_| {
                                div()
                                    .flex_shrink_0()
                                    .w(px(width))
                                    .h(px((width * 1.5).round()))
                                    .rounded(px(8.))
                                    .bg(ink(0.05))
                            }))
                        })
                        .children(items.iter().enumerate().map(|(index, media)| {
                            self.poster(media, width, cursor == Some(index), false, cx)
                        })),
                )
                .into_any_element(),
        )
    }

    /// A short eased glide for the shelf arrows; bounded and interruptible.
    fn scroll_shelf(&mut self, handle: &ScrollHandle, delta: f32, cx: &mut Context<Self>) {
        let start = handle.offset();
        // Scroll offsets are negative. Clamp to what the rail can actually show.
        let max = f32::from(handle.max_offset().x);
        let target = (f32::from(start.x) - delta).clamp(-max, 0.);
        if cx.reduce_motion() {
            handle.set_offset(point(px(target), start.y));
            cx.notify();
            return;
        }
        let handle = handle.clone();
        cx.spawn(async move |this, cx| {
            const STEPS: u32 = 18;
            let from = f32::from(start.x);
            for step in 1..=STEPS {
                let t = step as f32 / STEPS as f32;
                let eased = 1. - (1. - t).powi(4);
                handle.set_offset(point(px(from + (target - from) * eased), start.y));
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(14))
                    .await;
            }
        })
        .detach();
    }

    /// Wide cards, so what you were watching reads differently from what's new.
    fn continue_shelf(
        &self,
        viewport: f32,
        gutter: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entries = self.continue_watching();
        if entries.is_empty() {
            return None;
        }
        let width: f32 = if viewport < 900. { 220. } else { 268. };
        let height = (width * 9. / 16.).round();
        let cards: Vec<_> = entries
            .into_iter()
            .take(20)
            .map(|entry| {
                let art = self.art_path(
                    Art::Original,
                    entry
                        .media
                        .background
                        .as_ref()
                        .or(entry.media.poster.as_ref()),
                );
                let fraction = entry.fraction() as f32;
                let label = entry
                    .episode_label
                    .clone()
                    .unwrap_or_else(|| entry.media.kind_label().to_owned());
                let left = stimmy_core::playback::time_left(entry.seconds, entry.duration);
                let (name, id) = (entry.media.name.clone(), entry.id());
                let hover_media = entry.media.clone();
                let card_hovered =
                    self.hovered_poster.as_deref() == Some(entry.media.key().as_str());
                let accent = self.accent_for(&entry.media);
                let reduced = cx.reduce_motion();
                div()
                    .id(SharedString::from(format!("continue-{id}")))
                    .role(Role::Link)
                    .aria_label(format!("Continue {name}, {left}"))
                    .tab_index(0)
                    .w(px(width))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .cursor_pointer()
                    .rounded(px(8.))
                    .focus_visible(|style| style.bg(ink(0.14)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.poster_hovered(&hover_media, *hovered, cx);
                    }))
                    .relative()
                    .when(card_hovered && !reduced, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .w(px(width))
                                .h(px(height))
                                .child(beam(
                                    format!("continue-beam-{id}"),
                                    8.,
                                    accent,
                                    1100,
                                    false,
                                )),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.play_progress(&entry, cx)))
                    .child(
                        div()
                            .relative()
                            .h(px(height))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(ink(0.06))
                            .when_some(art, |el, path| {
                                el.child(img(path).size_full().object_fit(ObjectFit::Cover))
                            })
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(px(3.))
                                    .bg(ink(0.22))
                                    .child(div().h_full().w(relative(fraction)).bg(ink(0.96))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(tertiary())
                                    .child(format!("{label}  ·  {left}")),
                            ),
                    )
            })
            .collect();
        Some(
            div()
                .id("continue-shelf")
                .role(Role::Group)
                .aria_label("Continue watching")
                .relative()
                .flex()
                .flex_col()
                .gap(px(14.))
                .pb(px(30.))
                .child(
                    div()
                        .px(px(gutter))
                        .h(px(CONTROL_HEIGHT))
                        .flex()
                        .items_center()
                        .child(heading("continue-title", "Continue watching")),
                )
                .child(
                    div()
                        .id("continue-rail")
                        .flex()
                        .gap(px(POSTER_GAP))
                        .px(px(gutter))
                        .overflow_x_scroll()
                        .restrict_scroll_to_axis()
                        .children(cards),
                )
                .into_any_element(),
        )
    }

    pub fn home_page(
        &self,
        viewport: f32,
        height: f32,
        gutter: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let shelves: Vec<AnyElement> = (0..self.shelves.len())
            .filter_map(|row| self.shelf(row, viewport, gutter, cx))
            .collect();
        let resume = self.continue_shelf(viewport, gutter, cx);
        div()
            .relative()
            .flex()
            .flex_col()
            .pb(px(24.))
            .child(self.stage(viewport, height, gutter, cx))
            .children(resume)
            .children(shelves)
    }
}
