//! A title: full-bleed stage, seasons and episodes, and sources.

use super::{theme::*, widgets::*};
use crate::app::{Art, Load, Stimmy};
use gpui_kit::{component::IconName, prelude::*, *};
use stimmy_core::{
    DownloadState, Episode, Media, StreamAction, StreamResults, StreamSource,
    playback::{format_clock, up_next},
};

impl Stimmy {
    /// Where a video stands with downloading, as a control. `labelled` is for the
    /// title page; rows use just the icon.
    fn download_control(
        &self,
        id: SharedString,
        media: &Media,
        video: &str,
        labelled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let record = self.downloads.as_ref().and_then(|d| d.find(media, video));
        let to_downloads = cx.listener(|this, _: &ClickEvent, window, cx| {
            this.navigate(crate::app::Tab::Downloads, window, cx)
        });
        match record {
            Some(record) if record.state == DownloadState::Complete => if labelled {
                icon_label_button(id, IconName::Check, "Downloaded")
            } else {
                icon_button(id, IconName::Check, "Downloaded")
            }
            .on_click(to_downloads)
            .into_any_element(),
            Some(record) if record.state != DownloadState::Failed => {
                let percent = format!("{:.0}%", record.fraction() * 100.);
                quiet_button(id, &percent)
                    .on_click(to_downloads)
                    .into_any_element()
            }
            _ if self
                .preparing_downloads
                .contains(&format!("{}:{video}", media.key())) =>
            {
                busy_button(id, "Preparing…").into_any_element()
            }
            _ => {
                let (media, video) = (media.clone(), video.to_owned());
                let click = cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.download_video(media.clone(), video.clone(), None, cx)
                });
                if labelled {
                    svg_label_button(id, "stimmy/download.svg", "Download")
                        .on_click(click)
                        .into_any_element()
                } else {
                    svg_button(id, "stimmy/download.svg", "Download")
                        .on_click(click)
                        .into_any_element()
                }
            }
        }
    }

    fn source_row(&self, id: String, source: &StreamSource, cx: &mut Context<Self>) -> Div {
        let stream = &source.stream;
        // Add-ons commonly pack several lines of quality details into one label.
        let name = stream.label().replace('\n', " · ");
        let detail = stream.detail().replace('\n', " · ");
        let action = stream.action();
        let play = source.clone();
        let downloadable = self.detail.as_ref().map(|d| {
            let video = d.video.clone().unwrap_or_else(|| d.media.id.clone());
            (d.media.clone(), video, source.clone())
        });
        div()
            .py(px(12.))
            .border_t_1()
            .border_color(hairline())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(relative(1.45))
                            .text_color(secondary())
                            .line_clamp(2)
                            .child(if detail.is_empty() {
                                source.addon.clone()
                            } else {
                                format!("{}  ·  {detail}", source.addon)
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .map(|el| match action {
                        StreamAction::Direct(url) => {
                            let copy = url.to_owned();
                            el.when_some(downloadable.clone(), |el, (media, video, chosen)| {
                                el.child(
                                    svg_button(
                                        SharedString::from(format!("{id}-download")),
                                        "stimmy/download.svg",
                                        "Download this source",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.download_video(
                                                media.clone(),
                                                video.clone(),
                                                Some(chosen.clone()),
                                                cx,
                                            )
                                        },
                                    )),
                                )
                            })
                            .child(
                                icon_button(
                                    SharedString::from(format!("{id}-copy")),
                                    IconName::Copy,
                                    "Copy link",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                        this.announce("Link copied", cx);
                                    },
                                )),
                            )
                            .child(
                                primary_icon_button(
                                    SharedString::from(format!("{id}-play")),
                                    IconName::Play,
                                    "Play",
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.play_source(&play, cx)),
                                ),
                            )
                        }
                        StreamAction::Web(url) => {
                            let url = url.to_owned();
                            el.child(
                                icon_label_button(
                                    SharedString::from(format!("{id}-web")),
                                    IconName::ExternalLink,
                                    "Open in browser",
                                )
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                        }
                        StreamAction::Unsupported(reason) => el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(tertiary())
                                .child(reason),
                        ),
                    }),
            )
    }

    fn sources(&self, streams: &Load<StreamResults>, cx: &mut Context<Self>) -> Div {
        let installed = self.stream_addons_installed();
        div()
            .flex()
            .flex_col()
            .when(!installed, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .child(muted("Add a stream add-on to see sources for this title."))
                        .child(quiet_button("sources-add", "Add-ons").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.settings_page = crate::app::SettingsPage::Addons;
                                this.navigate(crate::app::Tab::Settings, window, cx);
                            },
                        ))),
                )
            })
            .when(installed, |el| match streams {
                Load::Idle => el,
                Load::Loading => el.child(muted("Looking for sources…")),
                Load::Ready(results) => {
                    let rows: Vec<Div> = results
                        .sources
                        .iter()
                        .enumerate()
                        .map(|(index, source)| {
                            self.source_row(format!("source-{index}"), source, cx)
                        })
                        .collect();
                    el.when(
                        results.sources.is_empty() && results.failures.is_empty(),
                        |el| el.child(muted("No sources found for this title.")),
                    )
                    .children(rows)
                    .when(!results.failures.is_empty(), |el| {
                        el.child(div().pt(px(10.)).child(muted(format!(
                            "Didn’t respond: {}",
                            results.failures.join(", ")
                        ))))
                    })
                }
            })
    }

    fn episode_row(
        &self,
        episode: &Episode,
        expanded: bool,
        streams: &Load<StreamResults>,
        cx: &mut Context<Self>,
    ) -> Div {
        let thumbnail = self.art_path(Art::Original, episode.thumbnail.as_ref());
        let id = episode.id.clone();
        let label = format!(
            "Play episode {}, {}",
            episode.number(),
            episode.display_title()
        );
        let media = self
            .detail
            .as_ref()
            .map(|d| d.media.clone())
            .unwrap_or_default();
        let (play_media, play_id, toggle_id) = (media.clone(), id.clone(), id.clone());
        let download = self.download_control(
            SharedString::from(format!("episode-download-{id}")),
            &media,
            &id,
            false,
            cx,
        );
        let resume = self.detail.as_ref().and_then(|d| {
            self.profile
                .as_ref()?
                .progress_for(&episode.id, &d.media)
                .filter(|p| p.resumable())
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("episode-{id}")))
                    .role(Role::Button)
                    .aria_label(label)
                    .mx(px(-10.))
                    .p(px(10.))
                    .flex()
                    .gap(px(16.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .when(expanded, |el| el.bg(ink(0.06)))
                    .hover(|style| style.bg(ink(0.08)))
                    // A click plays. Choosing a source is the small button on the right.
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_video(play_media.clone(), Some(play_id.clone()), cx)
                    }))
                    .child(
                        div()
                            .w(px(152.))
                            .h(px(85.))
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .relative()
                            .overflow_hidden()
                            .bg(ink(0.06))
                            .when_some(thumbnail, |el, path| {
                                el.child(img(path).size_full().object_fit(ObjectFit::Cover))
                            })
                            .when_some(resume, |el, progress| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .h(px(3.))
                                        .bg(ink(0.22))
                                        .child(
                                            div()
                                                .h_full()
                                                .w(relative(progress.fraction() as f32))
                                                .bg(ink(0.96)),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .pt(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(10.))
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(
                                        div()
                                            .text_color(tertiary())
                                            .child(episode.number().to_string()),
                                    )
                                    .child(
                                        div().min_w_0().truncate().child(episode.display_title()),
                                    ),
                            )
                            .when_some(episode.synopsis().map(str::to_owned), |el, text| {
                                el.child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(relative(1.45))
                                        .text_color(secondary())
                                        .line_clamp(2)
                                        .child(text),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(2.))
                            .child(download)
                            .child(
                                icon_button(
                                    SharedString::from(format!("episode-sources-{id}")),
                                    if expanded {
                                        IconName::ChevronUp
                                    } else {
                                        IconName::ChevronDown
                                    },
                                    "Choose a source",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.select_video(toggle_id.clone(), cx),
                                )),
                            ),
                    ),
            )
            .when(expanded, |el| {
                el.child(
                    div()
                        .pl(px(168.))
                        .pb(px(10.))
                        .child(self.sources(streams, cx)),
                )
            })
    }

    pub fn detail_page(
        &self,
        media: &Media,
        viewport: f32,
        height: f32,
        gutter: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let detail = self.detail.as_ref().expect("detail page requires a title");
        let saved = self.saved(media);
        let save = media.clone();
        let imdb = media.imdb_url();
        let stage_height = (height * 0.62).clamp(400., 640.);
        let meta = meta_line(media);
        let seasons = media.seasons();
        let credit = |label: &str, people: &[String]| {
            (!people.is_empty()).then(|| {
                format!(
                    "{label} {}",
                    people
                        .iter()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
        };
        let credits = [
            credit("With", &media.cast),
            credit("Directed by", &media.director),
        ];
        let body_width = viewport - gutter * 2.;
        // Play means whatever comes next for this title, and it starts straight away.
        let latest = self.profile.as_ref().and_then(|p| p.latest_for(media));
        let next = up_next(media, latest.as_ref());
        let resume = next
            .as_deref()
            .and_then(|video| self.resume_point(media, video));
        let episode = next
            .as_deref()
            .filter(|_| media.is_series())
            .and_then(|video| media.videos.iter().find(|v| v.id == video))
            .map(|e| format!("S{} E{}", e.season.unwrap_or(0), e.number()));
        let play_label = match (resume, episode) {
            (Some(_), Some(episode)) => format!("Resume {episode}"),
            (None, Some(episode)) => format!("Play {episode}"),
            (Some(seconds), None) => format!("Resume from {}", format_clock(seconds)),
            (None, None) => "Play".to_owned(),
        };
        let play = media.clone();
        let film_download = (!media.is_series())
            .then(|| self.download_control("detail-download".into(), media, &media.id, true, cx));

        let mut sections = div()
            .relative()
            .px(px(gutter))
            .flex()
            .flex_col()
            .gap(px(18.))
            .w(px(viewport));

        if !media.is_series() {
            sections = sections
                .child(heading("sources-heading", "Sources"))
                .child(self.sources(&detail.streams, cx));
        } else if !seasons.is_empty() {
            let current = detail.season.unwrap_or(seasons[0]);
            let selected = seasons.iter().position(|s| *s == current).unwrap_or(0);
            let labels = seasons
                .iter()
                .map(|s| {
                    if *s == 0 {
                        "Specials".to_owned()
                    } else {
                        format!("Season {s}")
                    }
                })
                .collect();
            let picks = seasons.clone();
            let season_media = media.clone();
            let episodes = media.episodes(current);
            let rows: Vec<Div> = episodes
                .iter()
                .map(|episode| {
                    let expanded = detail.video.as_ref() == Some(&episode.id);
                    self.episode_row(episode, expanded, &detail.streams, cx)
                })
                .collect();
            sections = sections
                .child(
                    div()
                        .id("season-picker")
                        .max_w(px(body_width))
                        .overflow_x_scroll()
                        .restrict_scroll_to_axis()
                        .child(self.pill(
                            "season-pill",
                            "Season",
                            labels,
                            selected,
                            true,
                            None,
                            move |this, index, _, cx| {
                                this.pill_forward.insert("season-pill", index > selected);
                                this.select_season(picks[index], cx);
                            },
                            cx,
                        )),
                )
                .child(
                    div().flex().child(
                        svg_label_button(
                            "download-season",
                            "stimmy/download.svg",
                            "Download season",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.download_season(season_media.clone(), current, cx)
                        })),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .max_w(px(860.))
                        .children(rows),
                );
        }

        div()
            .relative()
            .flex()
            .flex_col()
            .pb(px(56.))
            .gap(px(26.))
            .child(
                div()
                    .relative()
                    .h(px(stage_height))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(self.stage_backdrop(
                        media,
                        None,
                        viewport,
                        if cx.reduce_motion() {
                            0.
                        } else {
                            (-f32::from(self.detail_scroll.offset().y)).max(0.) * 0.38
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .relative()
                            .px(px(gutter))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .max_w(px(640. + gutter))
                            .child(self.title_mark(media, false))
                            .when(!meta.is_empty(), |el| {
                                el.child(div().text_size(px(13.)).text_color(ink(0.82)).child(meta))
                            })
                            .when_some(media.description.clone(), |el, text| {
                                el.child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(relative(1.5))
                                        .text_color(ink(0.84))
                                        .child(text),
                                )
                            })
                            .children(credits.into_iter().flatten().map(|line| {
                                div().text_size(px(12.5)).text_color(ink(0.68)).child(line)
                            }))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .mt(px(6.))
                                    .child(
                                        primary_icon_button(
                                            "detail-play",
                                            IconName::Play,
                                            &play_label,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.play_title(play.clone(), cx)
                                            }),
                                        ),
                                    )
                                    .when_some(film_download, |el, control| el.child(control))
                                    .child(
                                        save_button(
                                            "detail-save",
                                            saved,
                                            "In Library",
                                            "Save",
                                            cx.reduce_motion(),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.toggle_save(save.clone(), cx)
                                            }),
                                        ),
                                    )
                                    .when_some(imdb, |el, url| {
                                        el.child(
                                            icon_label_button(
                                                "imdb",
                                                IconName::ExternalLink,
                                                "IMDb",
                                            )
                                            .on_click(move |_, _, cx| cx.open_url(&url)),
                                        )
                                    }),
                            ),
                    ),
            )
            .when(detail.loading, |el| {
                el.child(div().px(px(gutter)).child(muted("Loading details…")))
            })
            .when_some(detail.error.clone(), |el, error| {
                el.child(
                    div()
                        .relative()
                        .px(px(gutter))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(error_text(error))
                        .child(
                            quiet_button("detail-retry", "Try again")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        ),
                )
            })
            .child(sections)
    }
}
