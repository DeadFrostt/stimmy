//! Downloads: what's here, what's coming, and what needs a hand.

use super::{theme::*, widgets::*};
use crate::app::{Art, Stimmy, Tab};
use gpui_kit::{component::IconName, prelude::*, *};
use stimmy_core::{DownloadRecord, DownloadState};

const FILTERS: [&str; 3] = ["All", "In progress", "Ready"];

fn status_line(record: &DownloadRecord, speed: Option<f64>) -> String {
    let size = match record.total {
        Some(total) if record.state != DownloadState::Complete => {
            format!("{} of {}", human_size(record.written), human_size(total))
        }
        _ => human_size(record.written),
    };
    match record.state {
        DownloadState::Complete => format!("Ready  ·  {size}"),
        DownloadState::Queued => "Waiting".to_owned(),
        DownloadState::Paused if record.link_expired => "Finding a fresh link…".to_owned(),
        DownloadState::Paused => format!("Paused  ·  {size}"),
        DownloadState::Failed => record
            .error
            .clone()
            .unwrap_or_else(|| "Couldn’t finish".to_owned()),
        DownloadState::Downloading => {
            let percent = format!("{:.0}%", record.fraction() * 100.);
            let Some(speed) = speed.filter(|s| *s > 0.) else {
                return format!("{percent}  ·  {size}");
            };
            let rate = format!("{}/s", human_size(speed as u64));
            match record.total {
                Some(total) if total > record.written => {
                    let left = (total - record.written) as f64 / speed;
                    let eta = if left < 60. {
                        "under a minute left".to_owned()
                    } else if left < 3600. {
                        format!("{:.0} min left", left / 60.)
                    } else {
                        format!("{:.1} hr left", left / 3600.)
                    };
                    format!("{percent}  ·  {size}  ·  {rate}  ·  {eta}")
                }
                _ => format!("{percent}  ·  {size}  ·  {rate}"),
            }
        }
    }
}

impl Stimmy {
    fn download_row(&self, record: DownloadRecord, cx: &mut Context<Self>) -> Div {
        let poster = self.art_path(Art::Original, record.media.poster.as_ref());
        let speed = self.downloads.as_ref().and_then(|d| d.speed(&record.id));
        let id = record.id.clone();
        let status = status_line(&record, speed);
        let failed = record.state == DownloadState::Failed;
        let downloading = record.state == DownloadState::Downloading;
        let reduced = cx.reduce_motion();
        let accent = self.accent_for(&record.media);
        let confirming = self.removing_download.as_ref() == Some(&id);
        let (play, resume, pause_id, remove_id) =
            (record.clone(), record.clone(), id.clone(), id.clone());
        let file = self.downloads.as_ref().and_then(|d| d.local_file(&record));
        div()
            .py(px(12.))
            .border_b_1()
            .border_color(hairline())
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .relative()
                    .w(px(56.))
                    .h(px(84.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .size_full()
                            .rounded(px(6.))
                            .overflow_hidden()
                            .bg(ink(0.06))
                            .when_some(poster, |el, path| {
                                el.child(img(path).size_full().object_fit(ObjectFit::Cover))
                            }),
                    )
                    // Light travels around what's downloading, and stops when it does.
                    .when(downloading && !reduced, |el| {
                        el.child(super::motion::beam(
                            format!("download-beam-{id}"),
                            6.,
                            accent,
                            2000,
                            true,
                        ))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(record.media.name.clone()),
                    )
                    .when_some(record.episode_label.clone(), |el, label| {
                        el.child(
                            div()
                                .text_size(px(13.))
                                .text_color(secondary())
                                .truncate()
                                .child(label),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(relative(1.45))
                            .text_color(if failed { error_tint() } else { tertiary() })
                            .child(status),
                    )
                    .when(record.state != DownloadState::Complete && !failed, |el| {
                        el.child(
                            div()
                                .mt(px(4.))
                                .h(px(3.))
                                .rounded_full()
                                .bg(ink(0.14))
                                .overflow_hidden()
                                .child(if reduced {
                                    div()
                                        .h_full()
                                        .w(relative(record.fraction()))
                                        .bg(accent)
                                        .into_any_element()
                                } else {
                                    div()
                                        .h_full()
                                        .bg(accent)
                                        .with_spring(
                                            SharedString::from(format!("bar-{}", record.id)),
                                            SpringAnimation::new(spring_soft())
                                                .to(record.fraction()),
                                            |el, value: f32| el.w(relative(value)),
                                        )
                                        .into_any_element()
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .map(|el| match record.state {
                        DownloadState::Complete => el
                            .when_some(file, |el, file| {
                                el.child(
                                    icon_button(
                                        SharedString::from(format!("reveal-{id}")),
                                        IconName::FolderOpen,
                                        "Show in Finder",
                                    )
                                    .on_click(move |_, _, cx| cx.reveal_path(&file)),
                                )
                            })
                            .child(
                                primary_icon_button(
                                    SharedString::from(format!("play-{id}")),
                                    IconName::Play,
                                    "Play",
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.play_download(&play, cx)
                                    }),
                                ),
                            ),
                        DownloadState::Downloading | DownloadState::Queued => {
                            el.child(
                                icon_button(
                                    SharedString::from(format!("pause-{id}")),
                                    IconName::Pause,
                                    "Pause",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.pause_download(&pause_id, cx),
                                )),
                            )
                        }
                        DownloadState::Paused | DownloadState::Failed => {
                            el.child(
                                quiet_button(
                                    SharedString::from(format!("resume-{id}")),
                                    if failed { "Try again" } else { "Resume" },
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.resume_download(&resume, cx),
                                )),
                            )
                        }
                    })
                    .child(
                        if confirming {
                            quiet_button(
                                SharedString::from(format!("delete-{id}")),
                                "Confirm delete",
                            )
                        } else {
                            icon_button(
                                SharedString::from(format!("delete-{id}")),
                                IconName::Close,
                                "Delete download",
                            )
                        }
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_download(remove_id.clone(), cx)
                        })),
                    ),
            )
    }

    pub fn downloads_page(&self, gutter: f32, cx: &mut Context<Self>) -> Div {
        let records = self
            .downloads
            .as_ref()
            .map(|d| d.records())
            .unwrap_or_default();
        let ready = records
            .iter()
            .filter(|r| r.state == DownloadState::Complete)
            .count();
        let size: u64 = records.iter().map(|r| r.written).sum();
        let total = records.len();
        let shown: Vec<DownloadRecord> = records
            .into_iter()
            .filter(|r| match self.download_filter {
                1 => r.state != DownloadState::Complete,
                2 => r.state == DownloadState::Complete,
                _ => true,
            })
            .collect();
        let empty_filter = shown.is_empty() && total > 0;
        let rows: Vec<Div> = shown
            .into_iter()
            .map(|record| self.download_row(record, cx))
            .collect();
        let current = self.download_filter;
        div()
            .pt(px(TOP_BAR + 26.))
            .pb(px(48.))
            .px(px(gutter))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(860.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .when(total > 0, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(heading(
                                    "downloads-summary",
                                    format!("{ready} ready  ·  {} on this Mac", human_size(size)),
                                ))
                                .child(self.pill(
                                    "downloads-filter",
                                    "Show",
                                    FILTERS.iter().map(|f| (*f).to_owned()).collect(),
                                    current,
                                    true,
                                    None,
                                    move |this, index, _, _| {
                                        this.pill_forward.insert("downloads-filter", index > current);
                                        this.download_filter = index;
                                    },
                                    cx,
                                )),
                        )
                    })
                    .children(rows)
                    .when(empty_filter, |el| el.child(muted("Nothing here right now.")))
                    .when(total == 0, |el| {
                        el.child(
                            div()
                                .pt(px(120.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(10.))
                                .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Nothing downloaded yet"))
                                .child(muted("Download a film or an episode from its page and it’ll play here, even offline."))
                                .child(div().mt(px(10.)).child(primary_button("downloads-browse", "Browse Home").on_click(
                                    cx.listener(|this, _, window, cx| this.navigate(Tab::Home, window, cx)),
                                ))),
                        )
                    }),
            )
    }
}
