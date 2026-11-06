//! The pill: one component for main navigation, Settings, seasons, and filters.
//!
//! It has no fill and no blur. A single indicator marks the selection, with a
//! fainter one following the pointer. Both are stateful springs, so a new target
//! redirects them mid-flight instead of restarting.

use super::theme::*;
use crate::app::{KindFilter, SettingsPage, Stimmy, Tab};
use gpui_kit::{prelude::*, *};

const INSET: f32 = 3.;

/// Each segment is its label's real width plus even padding, so the space
/// around every label matches.
pub fn measured_widths(text: &WindowTextSystem, labels: &[String], compact: bool) -> Vec<f32> {
    let weight = font(".SystemUIFont");
    let weight = Font {
        weight: FontWeight::MEDIUM,
        ..weight
    };
    labels
        .iter()
        .map(|label| {
            let run = TextRun {
                len: label.len(),
                font: weight.clone(),
                color: ink(1.),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let width = f32::from(text.layout_line(label, px(12.), &[run], None).width);
            width.ceil() + if compact { 20. } else { 24. }
        })
        .collect()
}

/// (left edge, right inset) of a segment within the pill.
fn edges(widths: &[f32], index: usize, total: f32) -> (f32, f32) {
    let left: f32 = INSET + widths[..index].iter().sum::<f32>();
    (left, total - left - widths[index])
}

impl Stimmy {
    /// A spring-driven lozenge spanning `edges`, stretching along its direction
    /// of travel. Reduce Motion places it directly.
    #[allow(clippy::too_many_arguments)]
    fn lozenge(
        &self,
        id: &'static str,
        kind: &'static str,
        (left, right): (f32, f32),
        opacity: f32,
        fill: Hsla,
        height: f32,
        forward: bool,
        cx: &App,
    ) -> AnyElement {
        let base = div()
            .absolute()
            .top(px(INSET))
            .h(px(height))
            .rounded_full()
            .bg(fill);
        if cx.reduce_motion() {
            return base
                .left(px(left))
                .right(px(right))
                .opacity(opacity)
                .into_any_element();
        }
        let (left_spring, right_spring) = if forward {
            (spring_trail(), spring_lead())
        } else {
            (spring_lead(), spring_trail())
        };
        base.with_spring(
            SharedString::from(format!("{id}-{kind}-l")),
            SpringAnimation::new(left_spring).to(left),
            |el, value: f32| el.left(px(value)),
        )
        .with_spring(
            SharedString::from(format!("{id}-{kind}-r")),
            SpringAnimation::new(right_spring).to(right),
            |el, value: f32| el.map_element(|el| el.right(px(value))),
        )
        .with_spring(
            SharedString::from(format!("{id}-{kind}-o")),
            SpringAnimation::new(spring_soft()).to(opacity),
            |el, value: f32| {
                el.map_element(|el| el.map_element(|el| el.opacity(value.clamp(0., 1.))))
            },
        )
        .into_any_element()
    }

    /// `island` replaces the labels with a brief status and resizes the pill.
    #[allow(clippy::too_many_arguments)]
    pub fn pill(
        &self,
        id: &'static str,
        label: &'static str,
        labels: Vec<String>,
        selected: usize,
        compact: bool,
        island: Option<SharedString>,
        on_select: impl Fn(&mut Self, usize, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let widths = measured_widths(&self.text, &labels, compact);
        let height: f32 = if compact { 22. } else { 24. };
        let natural = widths.iter().sum::<f32>() + INSET * 2. + 2.;
        let island_width = island
            .as_ref()
            .map(|message| message.chars().count() as f32 * 6.3 + 34.);
        let width = island_width.unwrap_or(natural);
        let showing_island = island.is_some();
        let hover = self.pill_hover.get(id).copied();
        let hover_at = hover
            .or(self.pill_hover_last.get(id).copied())
            .unwrap_or(selected);
        let forward = self.pill_forward.get(id).copied().unwrap_or(true);
        let selected_edges = edges(&widths, selected, natural - 2.);
        let hover_edges = edges(&widths, hover_at, natural - 2.);
        let hover_forward = hover_at >= selected;

        let segments = div()
            .flex()
            .items_center()
            .p(px(INSET))
            .w(px(natural - 2.))
            .flex_shrink_0()
            .children(labels.iter().enumerate().map(|(index, text)| {
                let select = on_select.clone();
                div()
                    .id(SharedString::from(format!("{id}-{index}")))
                    .role(Role::Tab)
                    .aria_label(text.clone())
                    .aria_selected(index == selected)
                    .tab_index(0)
                    .w(px(widths[index]))
                    .h(px(height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if index == selected {
                        ink(0.98)
                    } else {
                        ink(hover.filter(|h| *h == index).map_or(0.62, |_| 0.86))
                    })
                    .focus_visible(|style| style.border_1().border_color(ink(0.8)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.pill_hover.insert(id, index);
                            this.pill_hover_last.insert(id, index);
                        } else if this.pill_hover.get(id) == Some(&index) {
                            this.pill_hover.remove(id);
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        select(this, index, window, cx);
                        cx.notify();
                    }))
                    .child(text.clone())
            }));

        let frame = div()
            .id(id)
            .role(Role::TabList)
            .aria_label(label)
            .relative()
            .flex_shrink_0()
            .h(px(height + INSET * 2. + 2.))
            .rounded_full()
            .overflow_hidden()
            .border_1()
            .border_color(hairline())
            .flex()
            .items_center()
            .justify_center()
            .when(!showing_island, |el| {
                el.child(self.lozenge(
                    id,
                    "hover",
                    hover_edges,
                    if hover.is_some() && hover != Some(selected) {
                        1.
                    } else {
                        0.
                    },
                    ink(0.075),
                    height,
                    hover_forward,
                    cx,
                ))
                .child(self.lozenge(
                    id,
                    "active",
                    selected_edges,
                    1.,
                    ink(0.17),
                    height,
                    forward,
                    cx,
                ))
                .child(segments)
            })
            .when_some(island, |el, message| {
                el.child(
                    div()
                        .id(SharedString::from(format!("{id}-island")))
                        .role(Role::Status)
                        .aria_label(message.clone())
                        .px(px(14.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ink(0.96))
                        .whitespace_nowrap()
                        .child(message.clone())
                        .with_animation(
                            SharedString::from(format!("{id}-island-{message}")),
                            Animation::new(std::time::Duration::from_millis(260))
                                .with_easing(ease_out_quint()),
                            |el, t| el.opacity(t),
                        ),
                )
            });

        if cx.reduce_motion() {
            frame.w(px(width)).into_any_element()
        } else {
            frame
                .with_spring(
                    SharedString::from(format!("{id}-width")),
                    SpringAnimation::new(spring_soft()).to(width),
                    |el, value: f32| el.w(px(value)),
                )
                .into_any_element()
        }
    }

    pub fn main_nav(&self, cx: &mut Context<Self>) -> AnyElement {
        // The count of what's still going sits in the label, so it shows from anywhere.
        let active = self
            .downloads
            .as_ref()
            .map_or(0, |d| d.records().iter().filter(|r| r.is_active()).count());
        let labels = Tab::ALL
            .iter()
            .map(|tab| match tab {
                Tab::Downloads if active > 0 => format!("Downloads {active}"),
                tab => tab.label().to_owned(),
            })
            .collect();
        self.pill(
            "main-nav",
            "Main navigation",
            labels,
            self.tab as usize,
            false,
            self.island.clone(),
            |this, index, window, cx| this.navigate(Tab::ALL[index], window, cx),
            cx,
        )
    }

    pub fn settings_nav(&self, cx: &mut Context<Self>) -> AnyElement {
        const PAGES: [(SettingsPage, &str); 4] = [
            (SettingsPage::General, "General"),
            (SettingsPage::Account, "Account"),
            (SettingsPage::Addons, "Add-ons"),
            (SettingsPage::About, "About"),
        ];
        let selected = PAGES
            .iter()
            .position(|(page, _)| *page == self.settings_page)
            .unwrap_or(0);
        self.pill(
            "settings-nav",
            "Settings sections",
            PAGES.iter().map(|(_, label)| (*label).to_owned()).collect(),
            selected,
            true,
            None,
            move |this, index, _, _| {
                let from = PAGES
                    .iter()
                    .position(|(p, _)| *p == this.settings_page)
                    .unwrap_or(0);
                this.pill_forward.insert("settings-nav", index > from);
                this.settings_page = PAGES[index].0;
            },
            cx,
        )
    }

    pub fn kind_filter(
        &self,
        id: &'static str,
        current: KindFilter,
        apply: fn(&mut Self, KindFilter),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = KindFilter::ALL
            .iter()
            .position(|f| *f == current)
            .unwrap_or(0);
        self.pill(
            id,
            "Show",
            KindFilter::ALL
                .iter()
                .map(|f| f.label().to_owned())
                .collect(),
            selected,
            true,
            None,
            move |this, index, _, _| {
                this.pill_forward.insert(id, index > selected);
                this.cursor = None;
                apply(this, KindFilter::ALL[index]);
            },
            cx,
        )
    }
}
