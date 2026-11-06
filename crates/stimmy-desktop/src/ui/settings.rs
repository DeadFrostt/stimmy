//! Settings: its own small pill, and quiet rows on the room's light.

use super::{theme::*, widgets::*};
use crate::app::{SettingsPage, Stimmy};
use gpui_kit::{prelude::*, *};
use std::path::PathBuf;

fn row(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .py(px(16.))
        .border_b_1()
        .border_color(hairline())
        .flex()
        .items_center()
        .justify_between()
        .gap(px(32.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.into()),
                )
                .child(muted(detail.into())),
        )
        .child(control)
}

impl Stimmy {
    fn general(&self, cx: &mut Context<Self>) -> Div {
        let refreshing = self.shelves.iter().any(|shelf| shelf.loading);
        let stale = self.shelves.iter().any(|shelf| shelf.stale);
        let cache = self
            .cache_bytes
            .map_or_else(|| "Measuring…".to_owned(), human_size);
        let shortcuts: [(&str, &str); 11] = [
            ("⌘1", "Home"),
            ("⌘2  or  ⌘F", "Search"),
            ("⌘3", "Library"),
            ("⌘4", "Downloads"),
            ("⌘,", "Settings"),
            ("⌘R", "Refresh"),
            ("⌘D", "Save the open title"),
            ("[  ]", "Previous or next featured title"),
            ("Arrows, Return", "Move between posters and open one"),
            ("Esc", "Back"),
            (
                "Space, ←, →, ↑, ↓, F, M",
                "In the player: pause, seek, volume, full screen, mute",
            ),
        ];
        let folder = self.data_dir.clone();
        div()
            .flex()
            .flex_col()
            .child(row(
                "Catalogs",
                if refreshing {
                    "Refreshing…"
                } else if stale {
                    "Couldn’t reach the server. Showing saved catalogs."
                } else {
                    "Saved on this Mac, so Home opens instantly and works offline."
                },
                quiet_button("settings-refresh", "Refresh").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.load_shelves(cx);
                    },
                )),
            ))
            .child(row(
                "Artwork cache",
                format!("{cache} on this Mac. Old artwork is removed past 800 MB."),
                quiet_button("settings-clear", "Clear").on_click(cx.listener(|this, _, _, cx| {
                    this.clear_cache(cx);
                })),
            ))
            .child(row(
                "Your data",
                "Your library, watch history, add-ons, and downloads.",
                quiet_button("settings-folder", "Show in Finder").on_click(move |_, _, cx| {
                    cx.open_with_system(&folder);
                }),
            ))
            .child(
                div()
                    .pt(px(26.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(heading("shortcuts-heading", "Keyboard"))
                    .child(div().flex().flex_col().gap(px(8.)).children(shortcuts.map(
                        |(keys, what)| {
                            div()
                                .flex()
                                .text_size(px(13.))
                                .gap(px(12.))
                                .child(
                                    div()
                                        .w(px(176.))
                                        .flex_shrink_0()
                                        .text_color(ink(0.92))
                                        .child(keys),
                                )
                                .child(div().text_color(secondary()).child(what))
                        },
                    ))),
            )
    }

    fn addons_page(&self, cx: &mut Context<Self>) -> Div {
        let installed = self
            .addons
            .as_ref()
            .map(|a| a.installed().to_vec())
            .unwrap_or_default();
        let capabilities = |addon: &stimmy_core::Addon| {
            let mut parts = vec![];
            if !addon.manifest.catalogs.is_empty() {
                parts.push("Catalogs");
            }
            if addon.manifest.provides_streams() {
                parts.push("Sources");
            }
            if parts.is_empty() {
                "Metadata".to_owned()
            } else {
                parts.join(" and ")
            }
        };
        let rows: Vec<Div> = installed
            .iter()
            .map(|addon| {
                let url = addon.transport_url.clone();
                let confirming = self.removing_addon.as_ref() == Some(&url);
                row(
                    addon.manifest.name.clone(),
                    capabilities(addon),
                    quiet_button(
                        SharedString::from(format!("remove-{}", addon.manifest.id)),
                        if confirming {
                            "Confirm removal"
                        } else {
                            "Remove"
                        },
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.remove_addon(url.clone(), cx)),
                    ),
                )
            })
            .collect();
        let account_rows: Vec<Div> = self
            .profile
            .as_ref()
            .map(|p| p.account_addons().to_vec())
            .unwrap_or_default()
            .iter()
            .filter(|addon| addon.manifest.id != "com.linvo.cinemeta")
            .map(|addon| {
                row(
                    addon.manifest.name.clone(),
                    format!("{}. From your account.", capabilities(addon)),
                    div(),
                )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&self.addon_input, "Add-on address"))
                    .child(if self.installing_addon {
                        busy_button("addon-add", "Adding…")
                    } else {
                        primary_button("addon-add", "Add")
                            .on_click(cx.listener(|this, _, _, cx| this.install_addon(cx)))
                    }),
            )
            .when_some(self.addon_error.clone(), |el, error| {
                el.child(div().pt(px(10.)).child(error_text(error)))
            })
            .child(
                div().pt(px(8.)).child(row(
                    "Cinemeta",
                    "Catalogs and details. Built in.",
                    div(),
                )),
            )
            .children(rows)
            .children(account_rows)
            .child(div().pt(px(16.)).child(muted(
                "Sources are listed on each title. Add-ons from your Stremio account are managed in Stremio.",
            )))
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Div {
        let active = self.active_session().cloned();
        let rows: Vec<Div> = self
            .accounts
            .iter()
            .map(|account| {
                let id = account.user.id.clone();
                let is_active = self.active_account.as_ref() == Some(&id);
                let (use_id, remove_id) = (id.clone(), id.clone());
                row(
                    account.user.display_name(),
                    if is_active {
                        format!(
                            "{}. In use.",
                            account.user.email.clone().unwrap_or_default()
                        )
                    } else {
                        account.user.email.clone().unwrap_or_default()
                    },
                    div()
                        .flex()
                        .gap(px(6.))
                        .when(!is_active, |el| {
                            el.child(
                                quiet_button(SharedString::from(format!("use-{id}")), "Use")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.switch_account(Some(use_id.clone()), cx)
                                    })),
                            )
                        })
                        .child(
                            quiet_button(SharedString::from(format!("signout-{id}")), "Sign out")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_account(remove_id.clone(), cx)
                                })),
                        ),
                )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .when(active.is_none(), |el| {
                el.child(div().pb(px(12.)).child(muted(
                    "Sign in with your Stremio account to bring in your add-ons, library, and watch history. The password goes only to Stremio; the session is kept in your Keychain.",
                )))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(div().flex().child(field(&self.email_input, "Email")))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(field(&self.password_input, "Password"))
                                .child(if self.signing_in {
                                    busy_button("sign-in", "Signing in…")
                                } else {
                                    primary_button("sign-in", "Sign in")
                                        .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx)))
                                }),
                        ),
                )
            })
            .when_some(self.account_error.clone(), |el, error| {
                el.child(div().pt(px(10.)).child(error_text(error)))
            })
            .children(rows)
            .when(active.is_some(), |el| {
                el.child(row(
                    "This Mac’s guest profile",
                    "A separate library with no account.",
                    quiet_button("use-guest", "Use").on_click(cx.listener(|this, _, _, cx| {
                        this.switch_account(None, cx)
                    })),
                ))
                .child(row(
                    "Sync",
                    if self.syncing {
                        "Reading from Stremio…"
                    } else {
                        "Add-ons and library come from your account. Nothing here changes it."
                    },
                    if self.syncing {
                        busy_button("sync", "Syncing…")
                    } else {
                        quiet_button("sync", "Sync now").on_click(cx.listener(|this, _, _, cx| {
                            this.sync_account(cx)
                        }))
                    },
                ))
            })
            .when(active.is_none() && !self.accounts.is_empty(), |el| {
                el.child(div().pt(px(12.)).child(muted("Using the guest profile. Choose an account above to switch.")))
            })
    }

    fn about(&self, _cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Stimmy for Mac"))
            .child(muted(format!("Version {}. Built with Rust and GPUI.", env!("CARGO_PKG_VERSION"))))
            .child(muted("Film and series data and artwork come from Cinemeta and any add-ons you install."))
            .child(muted("An independent client, not affiliated with Stremio. Stimmy hosts no content: everything you watch comes from add-ons you choose."))
            .child(div().pt(px(6.)).flex().child(
                quiet_button("licenses", "Licenses and credits").on_click(|_, _, cx| {
                    let bundled = cx.app_path().ok().map(|path| path.join("Contents/Resources/THIRD_PARTY_NOTICES.md"));
                    let path = bundled.filter(|path| path.is_file()).unwrap_or_else(|| {
                        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../THIRD_PARTY_NOTICES.md")
                    });
                    cx.open_with_system(&path);
                }),
            ))
    }

    pub fn settings_page_view(&self, gutter: f32, cx: &mut Context<Self>) -> Div {
        let content = match self.settings_page {
            SettingsPage::General => self.general(cx),
            SettingsPage::Account => self.account_page(cx),
            SettingsPage::Addons => self.addons_page(cx),
            SettingsPage::About => self.about(cx),
        };
        div()
            .pt(px(TOP_BAR + 26.))
            .pb(px(56.))
            .px(px(gutter))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(640.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(div().flex().child(self.settings_nav(cx)))
                    .child(content),
            )
    }
}
