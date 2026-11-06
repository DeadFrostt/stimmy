//! Search and Library: a query or a filter above a poster grid.

use super::{theme::*, widgets::*};
use crate::app::{SearchMode, Stimmy, Tab};
use gpui_kit::{
    component::{Icon, IconName, input::Input},
    prelude::*,
    *,
};
use stimmy_core::{Media, genres};

impl Stimmy {
    fn search_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.search_focused;
        // The light travels only while results are actually on their way.
        let searching = self.search.loading && !cx.reduce_motion();
        div()
            .relative()
            .w_full()
            .max_w(px(520.))
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(6.))
            .rounded_full()
            .border_1()
            .border_color(if focused { ink(0.42) } else { ink(0.16) })
            .bg(ink(0.05))
            .child(
                Icon::new(IconName::Search)
                    .size(px(15.))
                    .text_color(secondary()),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search_input)
                        .aria_label("Search films and series")
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false),
                ),
            )
            .when(searching, |el| {
                el.child(super::motion::beam(
                    "search-beam",
                    20.,
                    ink(0.9),
                    1500,
                    true,
                ))
            })
            .when(self.search.mode != SearchMode::Idle, |el| {
                el.child(
                    icon_button("search-clear", IconName::Close, "Clear search")
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
                )
            })
    }

    pub fn search_page(&self, viewport: f32, gutter: f32, cx: &mut Context<Self>) -> Div {
        let width = viewport - gutter * 2.;
        let items: Vec<Media> = self.visible_items();
        let status: Option<AnyElement> = match (&self.search.mode, self.search.loading) {
            (_, true) => None,
            (SearchMode::Idle, _) => None,
            (_, false) if self.search.error.is_some() => None,
            (SearchMode::Query(query), false) if self.search.items.is_empty() => Some(
                muted(format!(
                    "Nothing found for “{query}”. Try fewer words or another spelling."
                ))
                .into_any_element(),
            ),
            _ => None,
        };
        let header = match &self.search.mode {
            SearchMode::Genre(genre) => Some(genre.clone()),
            SearchMode::Query(_) if !self.search.items.is_empty() => {
                Some(format!("{} results", items.len()))
            }
            _ => None,
        };
        let genre_list = genres(&self.all_addons(), "movie");
        div()
            .px(px(gutter))
            .pt(px(TOP_BAR + 34.))
            .pb(px(48.))
            .flex()
            .flex_col()
            .gap(px(26.))
            .child(div().flex().justify_center().child(self.search_field(cx)))
            .when(self.search.loading, |el| {
                el.child(div().flex().justify_center().child(muted("Searching…")))
            })
            .when_some(self.search.error.clone(), |el, error| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .child(error_text(error))
                        .child(
                            primary_button("search-retry", "Try again")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        ),
                )
            })
            .when_some(status, |el, status| {
                el.child(div().flex().justify_center().child(status))
            })
            .when(self.search.mode == SearchMode::Idle, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(14.))
                        .pt(px(10.))
                        .child(muted("Or browse a genre"))
                        .child(
                            div()
                                .max_w(px(640.))
                                .flex()
                                .flex_wrap()
                                .justify_center()
                                .gap(px(6.))
                                .children(genre_list.into_iter().map(|genre| {
                                    let pick = genre.clone();
                                    chip(SharedString::from(format!("genre-{genre}")), &genre)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.browse_genre(pick.clone(), cx)
                                        }))
                                })),
                        ),
                )
            })
            .when(!self.search.items.is_empty() || header.is_some(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(heading(
                            "results-heading",
                            header.clone().unwrap_or_default(),
                        ))
                        .child(self.kind_filter(
                            "search-filter",
                            self.search_filter,
                            |this, f| this.search_filter = f,
                            cx,
                        )),
                )
            })
            .when(self.search.failed > 0, |el| {
                el.child(muted(
                    "Some sources didn’t respond, so these results may be incomplete.",
                ))
            })
            .when(!items.is_empty(), |el| {
                el.child(self.poster_grid(&items, width, cx))
            })
    }

    pub fn library_page(&self, viewport: f32, gutter: f32, cx: &mut Context<Self>) -> Div {
        let width = viewport - gutter * 2.;
        let items: Vec<Media> = self.visible_items();
        let total = self.library_items().len();
        div()
            .px(px(gutter))
            .pt(px(TOP_BAR + 26.))
            .pb(px(48.))
            .flex()
            .flex_col()
            .gap(px(26.))
            .when(total > 0, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(heading(
                            "library-count",
                            format!(
                                "{total} saved {}",
                                if total == 1 { "title" } else { "titles" }
                            ),
                        ))
                        .child(self.kind_filter(
                            "library-filter",
                            self.library_filter,
                            |this, f| this.library_filter = f,
                            cx,
                        )),
                )
                .child(self.poster_grid(&items, width, cx))
            })
            .when(total > 0 && items.is_empty(), |el| {
                el.child(muted("Nothing of that kind saved yet."))
            })
            .when(total == 0 && self.profile.is_some(), |el| {
                el.child(
                    div()
                        .pt(px(120.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(22.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Nothing saved yet"),
                        )
                        .child(muted("Save a title and it will wait for you here."))
                        .child(div().mt(px(10.)).child(
                            primary_button("library-browse", "Browse Home").on_click(cx.listener(
                                |this, _, window, cx| this.navigate(Tab::Home, window, cx),
                            )),
                        )),
                )
            })
    }
}
