//! The player: the film fills the window, and the controls appear over it.
//!
//! No panel or box holds them. A title and a timeline sit on soft scrims that
//! fade out with the controls, and while a video starts, its backdrop and title
//! art hold the screen so opening a film feels like the lights going down.

use super::{theme::*, widgets::*};
use crate::app::{Art, Bezel, PlayerMenu, Stimmy};
use gpui_kit::{
    component::{Icon, IconName},
    prelude::*,
    *,
};
use std::time::Duration;
use stimmy_core::playback::format_clock;

const SPEEDS: [f32; 6] = [0.5, 0.75, 1., 1.25, 1.5, 2.];

fn own_icon(path: &'static str, size_px: f32) -> Svg {
    svg().path(path).size(px(size_px)).text_color(ink(0.95))
}

/// A plain player control: quiet at rest, with a wash under the pointer.
fn transport_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .tab_index(0)
        .flex_shrink_0()
        .h(px(CONTROL_HEIGHT))
        .min_w(px(CONTROL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink(0.95))
        .border_1()
        .border_color(transparent_black())
        .hover(|style| style.bg(ink(0.16)))
        .focus_visible(|style| style.border_color(ink(0.9)))
        .active(|style| style.opacity(0.7))
        // Otherwise a click here also reaches the video and toggles pause.
        .occlude()
}

impl Stimmy {
    /// Holds the screen until the first frame: backdrop, title art, a quiet spinner.
    fn loading_stage(&self, viewport: f32, cx: &mut Context<Self>) -> Div {
        let state = self.player.as_ref().expect("player is open");
        let media = state.request.media.clone();
        let reduced = cx.reduce_motion();
        div()
            .absolute()
            .inset_0()
            .bg(rgb(CANVAS))
            .overflow_hidden()
            .when_some(self.light.clone(), |el, path| {
                el.child(
                    img(path)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
            })
            .when_some(
                self.art_path(Art::Matte, media.background.as_ref()),
                |el, path| {
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .w(px(viewport))
                            .child(super::motion::backdrop_image(path, viewport)),
                    )
                },
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(22.))
                    .when(state.error.is_none(), |el| {
                        el.child(self.title_mark(&media, true))
                            .when_some(state.status.clone(), |el, status| el.child(muted(status)))
                            .child(if reduced {
                                muted("Starting…").into_any_element()
                            } else {
                                svg()
                                    .path(gpui_kit::assets::IconName::LoaderCircle.path())
                                    .size(px(22.))
                                    .text_color(ink(0.8))
                                    .with_animation(
                                        "player-spinner",
                                        Animation::new(Duration::from_millis(900)).repeat(),
                                        |el, t| {
                                            el.with_transformation(Transformation::rotate(
                                                percentage(t),
                                            ))
                                        },
                                    )
                                    .into_any_element()
                            })
                    }),
            )
    }

    fn timeline(&self, width: f32, gutter: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.player.as_ref().expect("player is open");
        let duration = state.snapshot.duration;
        let position = state
            .scrub
            .or(state.seek_target)
            .unwrap_or(state.snapshot.position);
        let accent = self.accent_for(&state.request.media);
        let played = if duration > 0. {
            (position / duration).clamp(0., 1.) as f32
        } else {
            0.
        };
        let buffered = if duration > 0. {
            (state.snapshot.buffered / duration).clamp(0., 1.) as f32
        } else {
            0.
        };
        let active = state.timeline_hovered || state.scrub.is_some();
        let thickness = if active { 6. } else { 3. };
        let knob = if active { 14. } else { 0. };
        let hover = state.hover_x.filter(|_| active).map(|x| {
            let fraction = ((x - gutter) / width).clamp(0., 1.);
            (fraction, format_clock(fraction as f64 * duration))
        });
        let reduced = cx.reduce_motion();

        let track = div()
            .absolute()
            .left_0()
            .right_0()
            .rounded_full()
            .bg(ink(0.24))
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(buffered))
                    .bg(ink(0.26)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(played))
                    .bg(accent),
            );
        let track: AnyElement = if reduced {
            track
                .h(px(thickness))
                .top(px((20. - thickness) / 2.))
                .into_any_element()
        } else {
            track
                .with_spring(
                    "timeline-thickness",
                    SpringAnimation::new(spring_soft()).to(thickness),
                    |el, value: f32| el.h(px(value)).top(px((20. - value) / 2.)),
                )
                .into_any_element()
        };

        div()
            .id("timeline")
            .occlude()
            .role(Role::Slider)
            .aria_label("Playback position")
            .aria_numeric_value(position)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(duration.max(1.))
            .relative()
            .w(px(width))
            .h(px(20.))
            .cursor_pointer()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if let Some(state) = &mut this.player {
                    state.timeline_hovered = *hovered;
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.scrub_at(f32::from(event.position.x), gutter, width, cx);
                    cx.stop_propagation();
                }),
            )
            .child(track)
            .when(knob > 0., |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(width * played - knob / 2.))
                        .top(px((20. - knob) / 2.))
                        .size(px(knob))
                        .rounded_full()
                        .bg(ink(1.))
                        .shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.4),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(4.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                )
            })
            .when_some(hover, |el, (fraction, label)| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(22.))
                        .left(px((width * fraction - 26.).clamp(0., width - 52.)))
                        .w(px(52.))
                        .flex()
                        .justify_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ink(1.))
                        .child(label),
                )
            })
    }

    pub fn scrub_at(&mut self, x: f32, gutter: f32, width: f32, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        let fraction = ((x - gutter) / width).clamp(0., 1.) as f64;
        state.scrub = Some(fraction * state.snapshot.duration);
        cx.notify();
    }

    fn finish_scrub(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.player.as_mut().and_then(|state| state.scrub.take()) else {
            return;
        };
        self.seek_to(target, cx);
    }

    fn toggle_menu(&mut self, menu: PlayerMenu, cx: &mut Context<Self>) {
        self.player_activity(cx);
        if let Some(state) = &mut self.player {
            state.menu = if state.menu == Some(menu) {
                None
            } else {
                Some(menu)
            };
        }
        cx.notify();
    }

    /// Rows in a menu, each a choice with a check beside the current one.
    fn menu_rows(&self, menu: PlayerMenu, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let state = self.player.as_ref().expect("player is open");
        let row = |id: SharedString, label: String, checked: bool| {
            div()
                .id(id)
                .role(Role::MenuItem)
                .aria_label(label.clone())
                .tab_index(0)
                .h(px(CONTROL_HEIGHT))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(13.))
                .text_color(ink(0.95))
                .hover(|style| style.bg(ink(0.13)))
                .focus_visible(|style| style.bg(ink(0.13)))
                .child(div().w(px(15.)).flex_shrink_0().when(checked, |el| {
                    el.child(Icon::new(IconName::Check).size(px(14.)))
                }))
                .child(div().min_w_0().truncate().child(label))
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        match menu {
            PlayerMenu::Source => {
                for (index, source) in state.candidates.iter().enumerate() {
                    let name = source.stream.label().replace('\n', " · ");
                    rows.push(
                        row(
                            format!("source-{index}").into(),
                            format!("{name}  ·  {}", source.addon),
                            index == state.candidate,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.switch_source(index, cx)))
                        .into_any_element(),
                    );
                }
            }
            PlayerMenu::Speed => {
                for speed in SPEEDS {
                    let label = if speed == 1. {
                        "Normal".to_owned()
                    } else {
                        format!("{speed}×")
                    };
                    rows.push(
                        row(format!("speed-{speed}").into(), label, state.speed == speed)
                            .on_click(cx.listener(move |this, _, _, cx| this.set_speed(speed, cx)))
                            .into_any_element(),
                    );
                }
            }
            PlayerMenu::Audio => {
                for track in &state.snapshot.audio {
                    let id = track.id.clone();
                    rows.push(
                        row(
                            format!("audio-{id}").into(),
                            track.name.clone(),
                            track.selected,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select_audio(id.clone(), cx)),
                        )
                        .into_any_element(),
                    );
                }
            }
            PlayerMenu::Subtitles => {
                let embedded_on = state.snapshot.subtitles.iter().any(|t| t.selected);
                rows.push(
                    row(
                        "subtitle-off".into(),
                        "Off".to_owned(),
                        !embedded_on && state.cues.is_none(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.subtitles_off(cx)))
                    .into_any_element(),
                );
                for track in &state.snapshot.subtitles {
                    let id = track.id.clone();
                    rows.push(
                        row(
                            format!("subtitle-{id}").into(),
                            track.name.clone(),
                            track.selected,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_engine_subtitle(id.clone(), cx)
                        }))
                        .into_any_element(),
                    );
                }
                let loaded = state.cues.as_ref().map(|c| c.url.clone());
                for (index, track) in state.addon_subtitles.iter().enumerate().take(60) {
                    let url = track.url.clone();
                    rows.push(
                        row(
                            format!("addon-subtitle-{index}").into(),
                            track.label(),
                            loaded.as_ref() == Some(&url),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_addon_subtitle(url.clone(), cx)
                        }))
                        .into_any_element(),
                    );
                }
            }
        }
        rows
    }

    fn menu_panel(&self, menu: PlayerMenu, right: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.menu_rows(menu, cx);
        div()
            .id("player-menu")
            .role(Role::Menu)
            .absolute()
            .right(px(right))
            .bottom(px(84.))
            .w(px(240.))
            .max_h(px(320.))
            .p(px(5.))
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .rounded(px(13.))
            // The one solid surface here: text over a moving picture needs it.
            .bg(hsla(0., 0., 0.07, 0.94))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.45),
                offset: point(px(0.), px(8.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .occlude()
            .children(rows)
    }

    pub fn player_page(
        &self,
        viewport: f32,
        height: f32,
        gutter: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let state = self.player.as_ref().expect("player is open");
        let media = state.request.media.clone();
        let snapshot = &state.snapshot;
        let visible = state.controls_visible || state.error.is_some();
        let width = viewport - gutter * 2.;
        let paused = snapshot.paused;
        let position = state
            .scrub
            .or(state.seek_target)
            .unwrap_or(snapshot.position);
        let waiting = !snapshot.first_frame || state.engine.is_none();
        let buffering_midway = snapshot.buffering && snapshot.first_frame && !paused;
        let next = self.next_video();
        let near_end = snapshot.ended
            || (snapshot.duration > 0. && snapshot.duration - position < 25. && position > 1.);
        let reduced = cx.reduce_motion();
        let controls_opacity = if visible { 1. } else { 0. };

        let scrim = |top: bool, size: f32| {
            let (from, to) = if top { (0.62, 0.) } else { (0., 0.76) };
            div()
                .absolute()
                .left_0()
                .right_0()
                .h(px(size))
                .when(top, |el| el.top_0())
                .when(!top, |el| el.bottom_0())
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(hsla(0., 0., 0., from), 0.),
                    linear_color_stop(hsla(0., 0., 0., to), 1.),
                ))
        };

        let title_row = div()
            .absolute()
            .top(px(11.))
            .left(px(if state.controls_visible { 84. } else { gutter }))
            .right(px(gutter))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                floating_icon_button("player-close", IconName::ChevronLeft, "Close player")
                    .on_click(cx.listener(|this, _, _, cx| this.close_player(cx))),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(media.name.clone()),
                    )
                    .when_some(state.request.episode_label.clone(), |el, label| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(ink(0.78))
                                .truncate()
                                .child(label),
                        )
                    }),
            );

        let can_pick_audio = snapshot.audio.len() > 1;
        let can_pick_source = state.candidates.len() > 1;
        let can_pick_subs = !snapshot.subtitles.is_empty() || !state.addon_subtitles.is_empty();
        let fullscreen_icon = IconName::Maximize;
        let volume_shown = if state.muted { 0. } else { state.volume };
        let clock = if snapshot.duration > 0. {
            format!(
                "{} / {}",
                format_clock(position),
                format_clock(snapshot.duration)
            )
        } else {
            format_clock(position)
        };
        let menu = state.menu;
        let speed = state.speed;

        let transport = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        transport_button("player-toggle", if paused { "Play" } else { "Pause" })
                            .child(
                                Icon::new(if paused {
                                    IconName::Play
                                } else {
                                    IconName::Pause
                                })
                                .size(px(18.)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                    )
                    .child(
                        transport_button("player-back-10", "Back 10 seconds")
                            .px(px(8.))
                            .child("−10")
                            .on_click(cx.listener(|this, _, _, cx| this.seek_by(-10., cx))),
                    )
                    .child(
                        transport_button("player-forward-10", "Forward 10 seconds")
                            .px(px(8.))
                            .child("+10")
                            .on_click(cx.listener(|this, _, _, cx| this.seek_by(10., cx))),
                    )
                    .child(
                        div()
                            .ml(px(10.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(ink(0.92))
                            .child(clock),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(can_pick_source, |el| {
                        el.child(
                            transport_button("player-source", "Source")
                                .when(menu == Some(PlayerMenu::Source), |el| el.bg(ink(0.16)))
                                .child(own_icon("stimmy/source.svg", 18.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_menu(PlayerMenu::Source, cx)
                                })),
                        )
                    })
                    .when(can_pick_subs, |el| {
                        el.child(
                            transport_button("player-subtitles", "Subtitles")
                                .when(menu == Some(PlayerMenu::Subtitles), |el| el.bg(ink(0.16)))
                                .child(own_icon("stimmy/captions.svg", 18.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_menu(PlayerMenu::Subtitles, cx)
                                })),
                        )
                    })
                    .when(can_pick_audio, |el| {
                        el.child(
                            transport_button("player-audio", "Audio")
                                .when(menu == Some(PlayerMenu::Audio), |el| el.bg(ink(0.16)))
                                .child(own_icon("stimmy/audio.svg", 18.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_menu(PlayerMenu::Audio, cx)
                                })),
                        )
                    })
                    .child(
                        transport_button("player-speed", "Playback speed")
                            .px(px(8.))
                            .when(menu == Some(PlayerMenu::Speed), |el| el.bg(ink(0.16)))
                            .child(if speed == 1. {
                                "1×".to_owned()
                            } else {
                                format!("{speed}×")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_menu(PlayerMenu::Speed, cx)
                            })),
                    )
                    .child(
                        transport_button(
                            "player-mute",
                            if volume_shown == 0. { "Unmute" } else { "Mute" },
                        )
                        .ml(px(4.))
                        .child(own_icon(
                            if volume_shown == 0. {
                                "stimmy/mute.svg"
                            } else {
                                "stimmy/volume.svg"
                            },
                            18.,
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                    )
                    .child(
                        div()
                            .id("player-volume")
                            .occlude()
                            .role(Role::Slider)
                            .aria_label("Volume")
                            .aria_numeric_value(volume_shown as f64)
                            .aria_min_numeric_value(0.)
                            .aria_max_numeric_value(1.)
                            .relative()
                            .w(px(84.))
                            .h(px(20.))
                            .mr(px(6.))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    let from_right =
                                        viewport - gutter - f32::from(event.position.x);
                                    let volume = 1. - ((from_right - VOLUME_RIGHT) / 84.);
                                    this.set_volume(volume, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(px(8.5))
                                    .h(px(3.))
                                    .rounded_full()
                                    .bg(ink(0.24))
                                    .overflow_hidden()
                                    .child(div().h_full().w(relative(volume_shown)).bg(ink(0.95))),
                            ),
                    )
                    .child(
                        transport_button("player-fullscreen", "Full screen")
                            .child(Icon::new(fullscreen_icon).size(px(16.)))
                            .on_click(|_, window, _| window.toggle_fullscreen()),
                    ),
            );

        let up_next = next.filter(|_| near_end).map(|_| {
            div().absolute().right(px(gutter)).bottom(px(92.)).child(
                primary_icon_button("player-next", IconName::Play, "Next episode")
                    .on_click(cx.listener(|this, _, _, cx| this.play_next(cx))),
            )
        });

        let controls = div()
            .absolute()
            .inset_0()
            .child(scrim(true, 96.))
            .child(scrim(false, 190.))
            .child(title_row)
            .when(paused && !waiting, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(gutter))
                        .bottom(px(86.))
                        .max_w(px(520.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(self.title_mark(&media, false))
                        .when_some(media.description.clone(), |el, text| {
                            el.child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(relative(1.5))
                                    .text_color(ink(0.86))
                                    .line_clamp(2)
                                    .child(text),
                            )
                        }),
                )
            })
            .when(state.error.is_none(), |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(gutter))
                        .right(px(gutter))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(self.timeline(width, gutter, cx))
                        .child(transport)
                        // Sinks away as the controls fade, and rises back with them.
                        .with_spring(
                            "player-transport-rise",
                            SpringAnimation::new(spring_soft()).to(if visible || reduced {
                                16.
                            } else {
                                2.
                            }),
                            |el, value: f32| el.bottom(px(value)),
                        ),
                )
            })
            .children(up_next)
            .when_some(menu, |el, menu| {
                // Each menu's edge lines up with its button, counted in from the
                // right: full screen, volume, mute, then speed, audio, subtitles.
                let speed_edge = CONTROL_HEIGHT + 2. + 84. + 6. + 2. + CONTROL_HEIGHT + 4. + 2.;
                let speed_width = 34.;
                let audio_edge = speed_edge + speed_width + 2.;
                let audio_width = if can_pick_audio {
                    CONTROL_HEIGHT + 2.
                } else {
                    0.
                };
                let right = gutter
                    + match menu {
                        PlayerMenu::Speed => speed_edge,
                        PlayerMenu::Audio => audio_edge,
                        PlayerMenu::Subtitles => audio_edge + audio_width,
                        PlayerMenu::Source => {
                            audio_edge
                                + audio_width
                                + if can_pick_subs {
                                    CONTROL_HEIGHT + 2.
                                } else {
                                    0.
                                }
                        }
                    };
                el.child(self.menu_panel(menu, right, cx))
            });
        let controls: AnyElement = if reduced {
            controls.opacity(controls_opacity).into_any_element()
        } else {
            controls
                .with_spring(
                    "player-controls",
                    SpringAnimation::new(spring_soft()).to(controls_opacity),
                    |el, value: f32| el.opacity(value.clamp(0., 1.)),
                )
                .into_any_element()
        };

        div()
            .id("player")
            .role(Role::Application)
            .aria_label(format!("Playing {}", media.name))
            .relative()
            .size_full()
            .overflow_hidden()
            .text_color(ink(1.))
            .font_family(".SystemUIFont")
            .text_size(px(14.))
            .key_context("Stimmy")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &crate::app::actions::Back, window, cx| {
                    this.back(window, cx)
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "space" | "k" => this.toggle_pause(cx),
                    "left" => this.seek_by(-10., cx),
                    "right" => this.seek_by(10., cx),
                    "up" => {
                        let volume = this.player.as_ref().map_or(1., |s| s.volume);
                        this.set_volume(volume + 0.05, cx);
                    }
                    "down" => {
                        let volume = this.player.as_ref().map_or(1., |s| s.volume);
                        this.set_volume(volume - 0.05, cx);
                    }
                    "m" => this.toggle_mute(cx),
                    "f" => window.toggle_fullscreen(),
                    "c" => this.toggle_menu(PlayerMenu::Subtitles, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let x = f32::from(event.position.x);
                if let Some(state) = &mut this.player {
                    state.hover_x = Some(x);
                }
                this.player_activity(cx);
                if this.player.as_ref().is_some_and(|s| s.scrub.is_some()) {
                    this.scrub_at(x, gutter, width, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_scrub(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_scrub(cx)),
            )
            // Clicking the picture pauses; double-clicking goes full screen.
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if this.player.as_ref().is_some_and(|s| s.menu.is_some()) {
                    if let Some(state) = &mut this.player {
                        state.menu = None;
                    }
                    cx.notify();
                } else if event.click_count() == 2 {
                    window.toggle_fullscreen();
                } else {
                    this.toggle_pause(cx);
                }
            }))
            // The drag region goes first so everything else sits on top of it.
            .child(
                div()
                    .id("player-drag")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(TOP_BAR))
                    .window_control_area(WindowControlArea::Drag),
            )
            .when(waiting, |el| el.child(self.loading_stage(viewport, cx)))
            .when(buffering_midway, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if reduced {
                            muted("Buffering…").into_any_element()
                        } else {
                            svg()
                                .path(gpui_kit::assets::IconName::LoaderCircle.path())
                                .size(px(30.))
                                .text_color(ink(0.86))
                                .with_animation(
                                    "player-buffering",
                                    Animation::new(Duration::from_millis(900)).repeat(),
                                    |el, t| {
                                        el.with_transformation(Transformation::rotate(percentage(
                                            t,
                                        )))
                                    },
                                )
                                .into_any_element()
                        }),
                )
            })
            .when_some(
                state.bezel.clone().filter(|_| !waiting),
                |el, (generation, bezel)| {
                    let content = match bezel {
                        Bezel::Play => Icon::new(IconName::Play).size(px(26.)).into_any_element(),
                        Bezel::Pause => Icon::new(IconName::Pause).size(px(26.)).into_any_element(),
                        Bezel::Text(text) => div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(text)
                            .into_any_element(),
                    };
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(hsla(0., 0., 0., 0.48))
                                    .child(content)
                                    .with_animation(
                                        SharedString::from(format!("bezel-{generation}")),
                                        Animation::new(Duration::from_millis(560))
                                            .with_easing(ease_out_quint()),
                                        move |el, t| {
                                            let size = if reduced { 74. } else { 62. + 26. * t };
                                            el.size(px(size)).opacity((1. - t).powf(1.4))
                                        },
                                    ),
                            ),
                    )
                },
            )
            .when_some(state.caption.clone(), |el, caption| {
                el.child(
                    div()
                        .absolute()
                        .left(px(gutter))
                        .right(px(gutter))
                        .bottom(px(if visible { 104. } else { 44. }))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .max_w(px(760.))
                                .px(px(12.))
                                .py(px(6.))
                                .rounded(px(7.))
                                .bg(hsla(0., 0., 0., 0.66))
                                .text_size(px((height * 0.027).clamp(18., 30.)))
                                .line_height(relative(1.28))
                                .font_weight(FontWeight::MEDIUM)
                                .text_center()
                                .child(caption),
                        ),
                )
            })
            .child(controls)
            .when_some(state.error.clone(), |el, error| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .occlude()
                        .child(
                            div()
                                .max_w(px(440.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(14.))
                                .text_center()
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("This video couldn’t play"),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(relative(1.5))
                                        .text_color(ink(0.84))
                                        .child(error),
                                )
                                .child(
                                    primary_button("player-error-close", "Choose another source")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.choose_source(cx)),
                                        ),
                                ),
                        ),
                )
            })
    }
}

/// Distance from the volume slider's right edge to the page gutter: the width of
/// the full-screen button and the slider's own margin.
const VOLUME_RIGHT: f32 = CONTROL_HEIGHT + 6.;
