//! Drawing. Colour comes from artwork; everything else is white at some alpha.

pub mod detail;
pub mod downloads;
pub mod home;
pub mod library;
pub mod motion;
pub mod pill;
pub mod player;
pub mod settings;
pub mod theme;
pub mod widgets;

use crate::app::{Stimmy, Tab, actions::*};
use gpui_kit::{prelude::*, *};
use theme::*;

impl Stimmy {
    /// The room's light: the previous title's colour holds underneath while the
    /// new one fades in over it, so a change never dips through black.
    fn ambient_layer(&self, cx: &mut Context<Self>) -> Div {
        const STRENGTH: f32 = 1.0;
        let generation = self.light_generation;
        let reduced = cx.reduce_motion();
        let layer = |path: std::path::PathBuf| {
            img(path)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
        };
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(rgb(CANVAS))
            .when_some(self.previous_light.clone(), |el, path| {
                el.child(layer(path).opacity(STRENGTH))
            })
            .when_some(self.light.clone(), |el, path| {
                if reduced {
                    el.child(layer(path).opacity(STRENGTH).into_any_element())
                } else {
                    el.child(
                        layer(path)
                            .with_animation(
                                SharedString::from(format!("light-{generation}")),
                                Animation::new(std::time::Duration::from_millis(620))
                                    .with_easing(ease_out_quint()),
                                move |image, t| image.opacity(t * STRENGTH),
                            )
                            .into_any_element(),
                    )
                }
            })
            // Pools toward the bottom of the window, so long pages settle down.
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(hsla(0., 0., 0., 0.), 0.35),
                linear_color_stop(hsla(0., 0., 0., 0.62), 1.),
            )))
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TOP_BAR))
            .flex()
            .items_center()
            .justify_center()
            // Keeps labels legible over bright artwork scrolling beneath. The
            // pill itself stays transparent.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(TOP_BAR + 26.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(hsla(0., 0., 0., 0.78), 0.),
                        linear_color_stop(hsla(0., 0., 0., 0.), 1.),
                    )),
            )
            .child(
                div()
                    .id("window-drag")
                    .absolute()
                    .inset_0()
                    .window_control_area(WindowControlArea::Drag)
                    // Moves only once dragging starts, so a plain click or a
                    // double click to zoom doesn't begin a window move.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, _| {
                            if event.click_count == 2 {
                                window.titlebar_double_click();
                            } else {
                                this.drag_armed = true;
                            }
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.drag_armed = false),
                    )
                    .on_mouse_move(cx.listener(|this, _, window, _| {
                        if std::mem::take(&mut this.drag_armed) {
                            window.start_window_move();
                        }
                    })),
            )
            .when(self.detail.is_some(), |el| {
                el.child(
                    div().absolute().left(px(84.)).top(px(11.)).child(
                        widgets::floating_icon_button(
                            "back",
                            component::IconName::ChevronLeft,
                            "Back",
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                    ),
                )
            })
            .child(self.main_nav(cx))
    }
}

impl Render for Stimmy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.clear_addon_input) {
            self.addon_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if std::mem::take(&mut self.clear_credentials) {
            self.email_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.password_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if self.host_view.is_none()
            && let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(&*window)
            && let raw_window_handle::RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            self.host_view = Some(crate::player::HostView(handle.ns_view.as_ptr() as *mut _));
        }
        // GPUI's layer only needs to let video through while the player is open.
        let wants_transparent = self.player.is_some();
        if wants_transparent != self.transparent {
            self.transparent = wants_transparent;
            window.set_background_appearance(if wants_transparent {
                WindowBackgroundAppearance::Transparent
            } else {
                WindowBackgroundAppearance::Opaque
            });
        }
        let viewport = f32::from(window.viewport_size().width);
        let height = f32::from(window.viewport_size().height);
        let gutter = if viewport < 900. { 28. } else { 48. };
        let width = viewport - gutter * 2.;
        let columns = widgets::grid_columns(width);
        let reduced = cx.reduce_motion();
        if self.player.is_some() {
            return self
                .player_page(viewport, height, gutter, cx)
                .into_any_element();
        }

        let page = if let Some(detail) = &self.detail {
            let media = detail.media.clone();
            self.detail_page(&media, viewport, height, gutter, cx)
                .into_any_element()
        } else {
            match self.tab {
                Tab::Home => self
                    .home_page(viewport, height, gutter, cx)
                    .into_any_element(),
                Tab::Search => self.search_page(viewport, gutter, cx).into_any_element(),
                Tab::Library => self.library_page(viewport, gutter, cx).into_any_element(),
                Tab::Downloads => self.downloads_page(gutter, cx).into_any_element(),
                Tab::Settings => self.settings_page_view(gutter, cx).into_any_element(),
            }
        };
        let detail_open = self.detail.is_some();
        let page_direction = if detail_open { 0. } else { self.page_direction };
        let page_key = SharedString::from(format!(
            "page-{}-{}",
            self.tab.label(),
            self.detail
                .as_ref()
                .map(|d| d.media.key())
                .unwrap_or_default()
        ));
        let scroll = if detail_open {
            &self.detail_scroll
        } else {
            &self.main_scroll
        };

        div()
            .id("stimmy")
            .role(Role::Application)
            .aria_label("Stimmy")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(CANVAS))
            .text_color(ink(1.))
            .font_family(".SystemUIFont")
            .text_size(px(14.))
            .key_context("Stimmy")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &OpenHome, w, cx| this.navigate(Tab::Home, w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenDownloads, w, cx| this.navigate(Tab::Downloads, w, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenSearch, w, cx| this.navigate(Tab::Search, w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenLibrary, w, cx| this.navigate(Tab::Library, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettings, w, cx| this.navigate(Tab::Settings, w, cx)),
            )
            .on_action(cx.listener(|this, _: &Back, w, cx| this.back(w, cx)))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &PreviousHero, _, cx| this.step_hero(false, cx)))
            .on_action(cx.listener(|this, _: &NextHero, _, cx| this.step_hero(true, cx)))
            .on_action(cx.listener(|_, _: &FocusNext, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &FocusPrevious, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|this, _: &ToggleSave, _, cx| {
                if let Some(media) = this.detail.as_ref().map(|d| d.media.clone()) {
                    this.toggle_save(media, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if this.focus.is_focused(window)
                    && !event.keystroke.modifiers.modified()
                    && this.move_cursor(event.keystroke.key.as_str(), columns, cx)
                {
                    cx.stop_propagation();
                }
            }))
            .child(self.ambient_layer(cx))
            .child(
                div()
                    .id(if detail_open {
                        "detail-scroll"
                    } else {
                        "main-scroll"
                    })
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    // Locks a trackpad gesture to one axis: without this, swiping
                    // sideways over a shelf also moves the page up and down.
                    .restrict_scroll_to_axis()
                    .track_scroll(scroll)
                    .when_some(self.notice.clone(), |el, notice| {
                        el.child(self.notice_banner(notice, gutter, cx))
                    })
                    .child(if reduced {
                        div().w_full().child(page).into_any_element()
                    } else {
                        // Pages arrive with a short, small settle.
                        div()
                            .w_full()
                            .child(page)
                            .with_animation(
                                page_key,
                                Animation::new(std::time::Duration::from_millis(240))
                                    .with_easing(ease_out_quint()),
                                // Tabs arrive from the direction you moved along
                                // the pill; a title rises into place.
                                move |el, t| {
                                    let left = page_direction * (1. - t) * 22.;
                                    let top = if page_direction == 0. {
                                        (1. - t) * 10.
                                    } else {
                                        0.
                                    };
                                    el.opacity(0.2 + 0.8 * t)
                                        .relative()
                                        .left(px(left))
                                        .top(px(top))
                                },
                            )
                            .into_any_element()
                    }),
            )
            .child(self.top_bar(cx))
            .into_any_element()
    }
}
