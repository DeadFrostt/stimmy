use gpui_kit::{prelude::*, *};
use stimmy_core::{StremioClient, data_directory, keychain};
use stimmy_desktop::app::{Stimmy, actions::*};

fn main() {
    let client = StremioClient::new().expect("HTTP client");
    // Debug builds: STIMMY_QA_DATA=<dir> runs an isolated instance for testing, with
    // its own data and no Keychain. Overriding HOME instead would break Keychain.
    let qa_dir = cfg!(debug_assertions)
        .then(|| std::env::var_os("STIMMY_QA_DATA"))
        .flatten();
    let keychain_service = qa_dir.is_none().then(|| keychain::SERVICE.to_owned());
    let qa = qa_dir.is_some();
    let data_dir = qa_dir.map_or_else(
        || data_directory().expect("application data folder"),
        Into::into,
    );
    gpui_kit::application()
        .with_assets(stimmy_desktop::AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            stimmy_desktop::apply_theme(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(vec![
                Menu::new("Stimmy").items([
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit Stimmy", Quit),
                ]),
                Menu::new("View").items([
                    MenuItem::action("Home", OpenHome),
                    MenuItem::action("Search", OpenSearch),
                    MenuItem::action("Library", OpenLibrary),
                    MenuItem::action("Downloads", OpenDownloads),
                    MenuItem::separator(),
                    MenuItem::action("Refresh", Refresh),
                ]),
            ]);
            stimmy_desktop::bind_keys(cx);
            cx.bind_keys([KeyBinding::new(stimmy_desktop::shortcut!("q"), Quit, None)]);
            let bounds = Bounds::centered(None, size(px(1280.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.), px(560.))),
                titlebar: Some(TitlebarOptions {
                    title: Some(if qa { "Stimmy QA" } else { "Stimmy" }.into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(19.))),
                }),
                app_id: Some("app.stimmy.gpui".into()),
                ..Default::default()
            };
            let (_, view) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Stimmy::new(client, data_dir, true, keychain_service, window, cx))
            })
            .expect("open the Stimmy window");
            cx.activate(true);
            #[cfg(not(debug_assertions))]
            let _ = view;
            #[cfg(debug_assertions)]
            if let Some(url) = qa.then(|| std::env::var("STIMMY_QA_PLAY").ok()).flatten() {
                view.update(cx, |app, cx| app.qa_play(url, cx));
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
}
