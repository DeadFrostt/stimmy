//! Application state and everything that isn't drawing.

pub mod actions {
    gpui_kit::actions!(
        stimmy,
        [
            Quit,
            OpenHome,
            OpenSearch,
            OpenLibrary,
            OpenSettings,
            OpenDownloads,
            Back,
            Refresh,
            ToggleSave,
            PreviousHero,
            NextHero,
            FocusNext,
            FocusPrevious
        ]
    );
}

use crate::player::{self, Engine, HostView, KeepAwake, Snapshot};
use gpui_kit::{
    component::input::{InputEvent, InputState},
    prelude::*,
    *,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};
use stimmy_core::{
    AccountSession, Addon, AddonStore, ArtworkCache, CatalogCache, Cue, DEFAULT_CACHE_LIMIT,
    DownloadEvent, DownloadRecord, Downloads, Error, Media, NewDownload, Pool, Profile,
    SearchResults, Shelf, StreamAction, StreamResults, StreamSource, StremioClient, SubtitleTrack,
    WatchProgress,
    account::now,
    cue_at, effective_addons, keychain,
    playback::{LastSource, episode_label, next_episode, rank_sources, season_of, up_next},
    shelves,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Home,
    Search,
    Library,
    Downloads,
    Settings,
}
impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Home,
        Tab::Search,
        Tab::Library,
        Tab::Downloads,
        Tab::Settings,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Library => "Library",
            Self::Downloads => "Downloads",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingsPage {
    General,
    Account,
    Addons,
    About,
}

/// Which titles a collection is showing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Films,
    Series,
}
impl KindFilter {
    pub const ALL: [KindFilter; 3] = [KindFilter::All, KindFilter::Films, KindFilter::Series];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Films => "Films",
            Self::Series => "Series",
        }
    }
    pub fn admits(self, media: &Media) -> bool {
        match self {
            Self::All => true,
            Self::Films => !media.is_series(),
            Self::Series => media.is_series(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Art {
    Original,
    Ambient,
    Matte,
    Logo,
    Accent,
}

/// What just happened in the player, flashed in the middle of the picture.
#[derive(Clone, PartialEq, Debug)]
pub enum Bezel {
    Play,
    Pause,
    Text(String),
}

pub struct ShelfState {
    pub shelf: Shelf,
    pub items: Vec<Media>,
    pub loading: bool,
    pub error: Option<String>,
    /// Showing the saved copy because the network request failed.
    pub stale: bool,
    pub scroll: ScrollHandle,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum SearchMode {
    #[default]
    Idle,
    Query(String),
    Genre(String),
}

#[derive(Default)]
pub struct SearchState {
    pub mode: SearchMode,
    pub items: Vec<Media>,
    pub failed: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
    pub debounce: u64,
}

pub enum Load<T> {
    Idle,
    Loading,
    Ready(T),
}

pub struct DetailState {
    pub media: Media,
    pub loading: bool,
    pub error: Option<String>,
    pub season: Option<u32>,
    /// The video whose sources are showing: the title itself for a film.
    pub video: Option<String>,
    pub streams: Load<StreamResults>,
}

/// What the player needs to know about a video.
#[derive(Clone)]
pub struct PlayRequest {
    pub media: Media,
    pub video_id: String,
    pub episode_label: Option<String>,
    pub url: String,
    pub filename: Option<String>,
    pub addon: String,
    pub subtitles: Vec<SubtitleTrack>,
    /// AVFoundation already failed, so this goes straight to libmpv.
    pub force_compatibility: bool,
    /// A downloaded file, so nothing here needs the network.
    pub local: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlayerMenu {
    Subtitles,
    Audio,
    Speed,
    Source,
}

/// A subtitle chosen from an add-on, drawn by Stimmy over either engine.
pub struct LoadedCues {
    pub url: String,
    pub cues: Vec<Cue>,
}

pub struct PlayerState {
    pub request: PlayRequest,
    pub engine: Option<Box<dyn Engine>>,
    pub snapshot: Snapshot,
    pub error: Option<String>,
    pub addon_subtitles: Vec<SubtitleTrack>,
    pub cues: Option<LoadedCues>,
    pub caption: Option<String>,
    pub controls_visible: bool,
    pub last_activity: std::time::Instant,
    pub scrub: Option<f64>,
    pub hover_x: Option<f32>,
    pub timeline_hovered: bool,
    pub volume: f32,
    pub muted: bool,
    pub speed: f32,
    pub menu: Option<PlayerMenu>,
    pub seek_target: Option<f64>,
    pub last_saved: f64,
    pub generation: u64,
    pub bezel: Option<(u64, Bezel)>,
    /// Playable sources, best guess first. Failure moves on to the next.
    pub candidates: Vec<StreamSource>,
    pub candidate: usize,
    pub opened_at: std::time::Instant,
    pub status: Option<String>,
    remembered: bool,
    subtitles_fetched: bool,
    _awake: KeepAwake,
}

/// A keyboard position: shelf and item on Home, 0 and item in a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

pub struct Stimmy {
    pub focus: FocusHandle,
    pub search_input: Entity<InputState>,
    pub addon_input: Entity<InputState>,
    pub client: StremioClient,
    pub artwork: ArtworkCache,
    pub catalog_cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pool: Pool,
    art_tx: async_channel::Sender<(Art, String, Option<PathBuf>)>,
    pub art: HashMap<(Art, String), PathBuf>,
    pub art_failed: HashSet<(Art, String)>,
    pub accents: HashMap<String, [u8; 3]>,
    /// -1, 0, or 1: which way the last tab change moved along the pill.
    pub page_direction: f32,
    pub previous_hero: Option<Media>,
    bezel_generation: u64,
    art_requested: HashSet<(Art, String)>,
    pub addons: Option<AddonStore>,
    pub profile: Option<Profile>,
    pub keychain_service: Option<String>,
    pub accounts: Vec<AccountSession>,
    pub active_account: Option<String>,
    pub email_input: Entity<InputState>,
    pub password_input: Entity<InputState>,
    pub signing_in: bool,
    pub account_error: Option<String>,
    pub syncing: bool,
    pub sync_generation: u64,
    pub text: std::rc::Rc<WindowTextSystem>,
    pub host_view: Option<HostView>,
    pub transparent: bool,
    pub player: Option<PlayerState>,
    pub downloads: Option<Downloads>,
    downloads_ticking: bool,
    pub download_filter: usize,
    pub removing_download: Option<String>,
    pub preparing_downloads: HashSet<String>,
    link_refreshes: HashMap<String, u32>,
    player_generation: u64,
    pub saved_volume: f32,
    pending_video: Option<String>,
    pub drag_armed: bool,
    pub clear_credentials: bool,
    pub tab: Tab,
    pub settings_page: SettingsPage,
    pub shelves: Vec<ShelfState>,
    pub hero: usize,
    pub search: SearchState,
    pub search_filter: KindFilter,
    pub library_filter: KindFilter,
    pub detail: Option<DetailState>,
    pub main_scroll: ScrollHandle,
    pub detail_scroll: ScrollHandle,
    pub light: Option<PathBuf>,
    pub previous_light: Option<PathBuf>,
    pub light_generation: u64,
    light_wanted: Option<String>,
    light_dwell: u64,
    pub hovered_poster: Option<String>,
    pub cursor: Option<Cursor>,
    pub island: Option<SharedString>,
    island_generation: u64,
    pub notice: Option<String>,
    pub pill_hover: HashMap<&'static str, usize>,
    pub pill_hover_last: HashMap<&'static str, usize>,
    pub pill_forward: HashMap<&'static str, bool>,
    pub search_focused: bool,
    pub clear_addon_input: bool,
    pub installing_addon: bool,
    pub addon_error: Option<String>,
    pub removing_addon: Option<String>,
    pub cache_bytes: Option<u64>,
}

/// A guest keeps the top-level folder, so a library from before accounts stays.
fn profile_dir(data_dir: &std::path::Path, account: Option<&str>) -> PathBuf {
    match account {
        Some(id) => data_dir
            .join("accounts")
            .join(format!("{:x}", stimmy_core::artwork::hash(id))),
        None => data_dir.to_path_buf(),
    }
}

fn read_active_account(data_dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(data_dir.join("active-account"))
        .ok()
        .map(|id| id.trim().to_owned())
}

fn write_active_account(data_dir: &std::path::Path, id: Option<&str>) {
    let _ = stimmy_core::store::atomic_write(
        &data_dir.join("active-account"),
        id.unwrap_or("").as_bytes(),
        false,
    );
}

/// Blocking network work on its own thread; the result returns to the UI thread.
/// A weak entity update discards it safely if the window has closed.
fn on_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    apply: impl FnOnce(&mut Stimmy, T, &mut Context<Stimmy>) + 'static,
    cx: &mut Context<Stimmy>,
) {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(work());
    });
    cx.spawn(async move |this, cx| {
        if let Ok(result) = receiver.recv().await {
            let _ = this.update(cx, |this, cx| {
                apply(this, result, cx);
                cx.notify();
            });
        }
    })
    .detach();
}

impl Stimmy {
    pub fn new(
        client: StremioClient,
        data_dir: PathBuf,
        load_catalogs: bool,
        keychain_service: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Films and series"));
        let addon_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste an add-on address"));
        cx.subscribe(
            &search_input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::Change => this.search_changed(cx),
                InputEvent::PressEnter { .. } => this.run_search(cx),
                InputEvent::Focus => {
                    this.search_focused = true;
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.search_focused = false;
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &addon_input,
            |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.install_addon(cx);
                }
            },
        )
        .detach();

        let email_input = cx.new(|cx| InputState::new(window, cx).placeholder("Email"));
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        cx.subscribe(
            &password_input,
            |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.sign_in(cx);
                }
            },
        )
        .detach();

        let mut notices = Vec::new();
        // Only someone who has signed in here needs the Keychain, so a guest is
        // never shown a macOS permission prompt.
        let known = data_dir.join("accounts-known").exists();
        let accounts = match keychain_service
            .as_deref()
            .filter(|_| known)
            .map(keychain::read)
        {
            Some(Ok(accounts)) => accounts,
            Some(Err(error)) => {
                notices.push(format!("Sign in again to use your account. {error}"));
                vec![]
            }
            _ => vec![],
        };
        let remembered = read_active_account(&data_dir);
        let active_account = remembered.filter(|id| accounts.iter().any(|a| a.user.id == *id));
        let profile = match Profile::open(profile_dir(&data_dir, active_account.as_deref())) {
            Ok(profile) => Some(profile),
            Err(error) => {
                notices.push(format!(
                    "Your library couldn’t be opened and is untouched. {error}"
                ));
                None
            }
        };
        let addons = match AddonStore::open(data_dir.join("addons.json")) {
            Ok(addons) => Some(addons),
            Err(error) => {
                notices.push(format!(
                    "Your add-ons couldn’t be opened and are untouched. {error}"
                ));
                None
            }
        };
        let downloads = match Downloads::open(data_dir.join("downloads")) {
            Ok(downloads) => Some(downloads),
            Err(error) => {
                notices.push(format!(
                    "Downloads couldn’t be opened and are untouched. {error}"
                ));
                None
            }
        };
        let artwork = ArtworkCache::new(client.clone(), data_dir.join("artwork"));
        let (art_tx, art_rx) = async_channel::unbounded();
        cx.spawn(async move |this, cx| {
            while let Ok((kind, source, path)) = art_rx.recv().await {
                if this
                    .update(cx, |this, cx| this.art_arrived(kind, source, path, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let pool = Pool::new(6);
        // Opening libmpv takes seconds the first time, so it happens before Play.
        pool.submit(|| {
            let _ = player::mpv_available();
        });
        {
            let artwork = artwork.clone();
            pool.submit(move || {
                let _ = artwork.evict(DEFAULT_CACHE_LIMIT);
            });
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            focus,
            search_input,
            addon_input,
            client,
            artwork,
            catalog_cache_dir: data_dir.join("catalogs"),
            data_dir,
            pool,
            art_tx,
            art: HashMap::new(),
            art_failed: HashSet::new(),
            accents: HashMap::new(),
            page_direction: 0.,
            previous_hero: None,
            bezel_generation: 0,
            art_requested: HashSet::new(),
            addons,
            profile,
            keychain_service,
            accounts,
            active_account,
            email_input,
            password_input,
            signing_in: false,
            account_error: None,
            syncing: false,
            sync_generation: 0,
            text: window.text_system().clone(),
            host_view: None,
            transparent: false,
            player: None,
            downloads,
            downloads_ticking: false,
            download_filter: 0,
            removing_download: None,
            preparing_downloads: HashSet::new(),
            link_refreshes: HashMap::new(),
            player_generation: 0,
            saved_volume: 1.,
            pending_video: None,
            drag_armed: false,
            clear_credentials: false,
            tab: Tab::Home,
            settings_page: SettingsPage::General,
            shelves: vec![],
            hero: 0,
            search: SearchState::default(),
            search_filter: KindFilter::All,
            library_filter: KindFilter::All,
            detail: None,
            main_scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            light: None,
            previous_light: None,
            light_generation: 0,
            light_wanted: None,
            light_dwell: 0,
            hovered_poster: None,
            cursor: None,
            island: None,
            island_generation: 0,
            notice: (!notices.is_empty()).then(|| notices.join(" ")),
            pill_hover: HashMap::new(),
            pill_hover_last: HashMap::new(),
            pill_forward: HashMap::new(),
            search_focused: false,
            clear_addon_input: false,
            installing_addon: false,
            addon_error: None,
            removing_addon: None,
            cache_bytes: None,
        };
        this.prefetch_library(cx);
        if load_catalogs {
            this.load_shelves(cx);
            this.sync_account(cx);
            // Anything interrupted by quitting carries on from where it was.
            if let Some(downloads) = &this.downloads {
                downloads.pump();
            }
            this.ensure_download_ticker(cx);
        }
        this
    }

    /// The account's add-ons, then this Mac's, with Cinemeta always present.
    pub fn all_addons(&self) -> Vec<Addon> {
        effective_addons(
            self.profile.as_ref().map_or(&[], |p| p.account_addons()),
            self.addons.as_ref().map_or(&[], |a| a.installed()),
        )
    }
    pub fn active_session(&self) -> Option<&AccountSession> {
        let id = self.active_account.as_ref()?;
        self.accounts.iter().find(|a| a.user.id == *id)
    }
    pub fn stream_addons_installed(&self) -> bool {
        self.all_addons()
            .iter()
            .any(|a| a.manifest.provides_streams())
    }

    // ---- Artwork -------------------------------------------------------------

    pub fn art_path(&self, kind: Art, source: Option<&String>) -> Option<PathBuf> {
        self.art.get(&(kind, source?.clone())).cloned()
    }
    pub fn art_gone(&self, kind: Art, source: Option<&String>) -> bool {
        source.is_none_or(|source| self.art_failed.contains(&(kind, source.clone())))
    }

    pub fn want_art(&mut self, kind: Art, source: &str) {
        let key = (kind, source.to_owned());
        if !self.art_requested.insert(key) {
            return;
        }
        let artwork = self.artwork.clone();
        let sender = self.art_tx.clone();
        let source = source.to_owned();
        self.pool.submit(move || {
            let result = match kind {
                Art::Original => artwork.original(&source),
                Art::Ambient => artwork.ambient(&source),
                Art::Matte => artwork.matte(&source),
                Art::Logo => artwork.logo(&source),
                Art::Accent => artwork.accent(&source),
            };
            let _ = sender.send_blocking((kind, source, result.ok()));
        });
    }

    fn art_arrived(
        &mut self,
        kind: Art,
        source: String,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        match path {
            Some(path) if kind == Art::Accent => {
                if let Ok(bytes) = std::fs::read(path)
                    && let [r, g, b] = bytes[..]
                {
                    self.accents.insert(source, [r, g, b]);
                }
            }
            Some(path) => {
                if kind == Art::Ambient && self.light_wanted.as_deref() == Some(source.as_str()) {
                    self.set_light(path.clone());
                }
                self.art.insert((kind, source), path);
            }
            None => {
                self.art_failed.insert((kind, source));
            }
        }
        cx.notify();
    }

    fn set_light(&mut self, path: PathBuf) {
        if self.light.as_ref() != Some(&path) {
            self.previous_light = self.light.take();
            self.light = Some(path);
            self.light_generation += 1;
        }
    }

    /// Lights the window from a title's artwork. Backdrops carry the truest
    /// colour; a poster stands in when a title has none.
    pub fn light_from(&mut self, media: &Media, cx: &mut Context<Self>) {
        let Some(source) = media.background.as_ref().or(media.poster.as_ref()).cloned() else {
            return;
        };
        self.light_wanted = Some(source.clone());
        if let Some(path) = self.art.get(&(Art::Ambient, source.clone())).cloned() {
            self.set_light(path);
            cx.notify();
        } else {
            self.want_art(Art::Ambient, &source);
        }
    }

    /// Hovering relights the room, after a short dwell so sweeping the pointer
    /// across a shelf doesn't strobe. Reduce Motion keeps the room still.
    /// The colour of light for a title, once it's known.
    pub fn accent_for(&self, media: &Media) -> Hsla {
        let source = media.background.as_ref().or(media.poster.as_ref());
        source
            .and_then(|source| self.accents.get(source))
            .map(|rgb| crate::ui::motion::accent_light(*rgb))
            .unwrap_or_else(crate::ui::motion::no_accent)
    }

    fn want_accent(&mut self, media: &Media) {
        if let Some(source) = media.background.as_ref().or(media.poster.as_ref()) {
            self.want_art(Art::Accent, &source.clone());
        }
    }

    pub fn poster_hovered(&mut self, media: &Media, hovered: bool, cx: &mut Context<Self>) {
        let key = media.key();
        if hovered {
            self.want_accent(media);
            self.hovered_poster = Some(key);
            self.light_dwell += 1;
            if cx.reduce_motion() {
                cx.notify();
                return;
            }
            let generation = self.light_dwell;
            let media = media.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(140))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.light_dwell == generation {
                        this.light_from(&media, cx);
                    }
                });
            })
            .detach();
        } else if self.hovered_poster.as_deref() == Some(key.as_str()) {
            self.hovered_poster = None;
            self.light_dwell += 1;
        }
        cx.notify();
    }

    fn prefetch_posters(&mut self, items: &[Media]) {
        for media in items {
            if let Some(poster) = &media.poster {
                self.want_art(Art::Original, poster);
            }
        }
    }
    fn prefetch_stage(&mut self, media: &Media) {
        self.want_accent(media);
        if let Some(background) = &media.background {
            self.want_art(Art::Matte, background);
            self.want_art(Art::Ambient, background);
        }
        if let Some(logo) = &media.logo {
            self.want_art(Art::Logo, logo);
        }
    }
    fn prefetch_library(&mut self, _cx: &mut Context<Self>) {
        let items = self.library_items();
        self.prefetch_posters(&items);
        for entry in self.continue_watching() {
            if let Some(background) = entry
                .media
                .background
                .as_ref()
                .or(entry.media.poster.as_ref())
            {
                self.want_art(Art::Original, background);
            }
        }
    }

    // ---- Home ----------------------------------------------------------------

    /// Paints saved catalogs straight away where they exist, then refreshes.
    /// A failed refresh keeps what's on screen instead of emptying the shelf.
    pub fn load_shelves(&mut self, cx: &mut Context<Self>) {
        let addons = self.all_addons();
        let wanted = shelves(&addons);
        let cache = CatalogCache::new(self.catalog_cache_dir.clone());
        let mut previous: Vec<ShelfState> = std::mem::take(&mut self.shelves);
        self.shelves = wanted
            .into_iter()
            .map(|shelf| {
                let existing = previous
                    .iter()
                    .position(|state| state.shelf.id == shelf.id)
                    .map(|index| previous.remove(index));
                match existing {
                    Some(mut state) => {
                        state.shelf = shelf;
                        state.loading = true;
                        state.error = None;
                        state
                    }
                    None => {
                        let items = cache.read(&shelf.id).unwrap_or_default();
                        ShelfState {
                            shelf,
                            items,
                            loading: true,
                            error: None,
                            stale: false,
                            scroll: ScrollHandle::new(),
                        }
                    }
                }
            })
            .collect();
        let saved: Vec<Media> = self.shelves.iter().flat_map(|s| s.items.clone()).collect();
        self.prefetch_posters(&saved);
        self.stage_changed(cx);
        for state in &self.shelves {
            let client = self.client.clone();
            let shelf = state.shelf.clone();
            let id = shelf.id.clone();
            let cache_dir = self.catalog_cache_dir.clone();
            on_thread(
                move || {
                    let result = client.catalog(&shelf.addon, &shelf.catalog, &[]);
                    if let Ok(items) = &result {
                        let _ = CatalogCache::new(cache_dir).write(&shelf.id, items);
                    }
                    result
                },
                move |this, result, cx| this.shelf_loaded(&id, result, cx),
                cx,
            );
        }
        cx.notify();
    }

    fn shelf_loaded(
        &mut self,
        id: &str,
        result: Result<Vec<Media>, Error>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.shelves.iter_mut().find(|s| s.shelf.id == id) else {
            return;
        };
        state.loading = false;
        match result {
            Ok(items) => {
                state.items = items.clone();
                state.stale = false;
                state.error = None;
                self.prefetch_posters(&items);
            }
            Err(error) => {
                state.stale = !state.items.is_empty();
                state.error = state.items.is_empty().then(|| error.to_string());
            }
        }
        self.stage_changed(cx);
        if self.shelves.iter().all(|s| !s.loading) && self.shelves.iter().any(|s| s.stale) {
            self.announce("Offline, showing saved catalogs", cx);
        }
    }

    /// The featured titles, alternating films and series, only with backdrops.
    pub fn stage_titles(&self) -> Vec<&Media> {
        let usable = |state: &'_ &ShelfState| !state.items.is_empty();
        let mut rails: Vec<_> = self
            .shelves
            .iter()
            .filter(usable)
            .take(2)
            .map(|state| {
                state
                    .items
                    .iter()
                    .filter(|m| m.background.is_some())
                    .take(3)
            })
            .collect();
        let mut titles = Vec::new();
        let mut seen = HashSet::new();
        'outer: loop {
            let mut progressed = false;
            for rail in &mut rails {
                if let Some(media) = rail.next() {
                    progressed = true;
                    if seen.insert(media.key()) {
                        titles.push(media);
                    }
                    if titles.len() == 5 {
                        break 'outer;
                    }
                }
            }
            if !progressed {
                break;
            }
        }
        titles
    }

    fn stage_changed(&mut self, cx: &mut Context<Self>) {
        let titles: Vec<Media> = self.stage_titles().into_iter().cloned().collect();
        if self.hero >= titles.len() {
            self.hero = 0;
        }
        for media in &titles {
            self.prefetch_stage(media);
        }
        if self.tab == Tab::Home
            && self.detail.is_none()
            && self.hovered_poster.is_none()
            && let Some(media) = titles.get(self.hero)
        {
            self.light_from(media, cx);
        }
    }

    pub fn show_hero(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.hero {
            self.previous_hero = self.stage_titles().get(self.hero).map(|m| (*m).clone());
        }
        self.hero = index;
        self.stage_changed(cx);
        cx.notify();
    }
    pub fn step_hero(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.stage_titles().len();
        if count > 0 && self.tab == Tab::Home && self.detail.is_none() {
            let next = if forward {
                (self.hero + 1) % count
            } else {
                (self.hero + count - 1) % count
            };
            self.show_hero(next, cx);
        }
    }

    // ---- Navigation -----------------------------------------------------------

    pub fn navigate(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        let (from, to) = (self.tab as usize, tab as usize);
        if from != to {
            self.pill_forward.insert("main-nav", to > from);
            self.page_direction = if to > from { 1. } else { -1. };
        }
        self.tab = tab;
        self.detail = None;
        self.cursor = None;
        self.main_scroll.set_offset(point(px(0.), px(0.)));
        if tab == Tab::Search {
            window.focus(&self.search_input.focus_handle(cx), cx);
        } else {
            window.focus(&self.focus, cx);
        }
        match tab {
            Tab::Home => self.stage_changed(cx),
            Tab::Search => {
                if let Some(media) = self.search.items.first().cloned() {
                    self.light_from(&media, cx);
                }
            }
            Tab::Library => {
                if let Some(media) = self.library_items().first().cloned() {
                    self.light_from(&media, cx);
                }
            }
            Tab::Downloads => {
                let records = self
                    .downloads
                    .as_ref()
                    .map(Downloads::records)
                    .unwrap_or_default();
                for record in records {
                    if let Some(poster) = &record.media.poster {
                        self.want_art(Art::Original, poster);
                    }
                }
            }
            Tab::Settings => self.measure_cache(cx),
        }
        cx.notify();
    }

    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.player.is_some() {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            } else {
                self.close_player(cx);
            }
            return;
        }
        if self.detail.take().is_some() {
            window.focus(&self.focus, cx);
            if self.tab == Tab::Home {
                self.stage_changed(cx);
            }
            cx.notify();
        } else if self.tab == Tab::Search && self.search.mode != SearchMode::Idle {
            self.clear_search(window, cx);
        }
    }

    // ---- Search ---------------------------------------------------------------

    fn search_query(&self, cx: &App) -> String {
        self.search_input.read(cx).value().trim().to_owned()
    }

    /// Search as you type, once typing pauses.
    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.search.debounce += 1;
        let debounce = self.search.debounce;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(280))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.search.debounce == debounce {
                    this.run_search(cx);
                }
            });
        })
        .detach();
    }

    pub fn run_search(&mut self, cx: &mut Context<Self>) {
        self.search.debounce += 1;
        let query = self.search_query(cx);
        let mode = if query.is_empty() {
            SearchMode::Idle
        } else {
            SearchMode::Query(query.clone())
        };
        if mode == self.search.mode {
            return;
        }
        if query.is_empty() {
            self.search.generation += 1;
            self.search.mode = SearchMode::Idle;
            self.search.items.clear();
            self.search.loading = false;
            self.search.error = None;
            self.search.failed = 0;
            cx.notify();
            return;
        }
        let client = self.client.clone();
        let addons = self.all_addons();
        self.begin_results(mode, move || client.search(&query, &addons), cx);
    }

    pub fn browse_genre(&mut self, genre: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let addons = self.all_addons();
        let term = genre.clone();
        self.begin_results(
            SearchMode::Genre(genre),
            move || {
                // Popular titles in both kinds, from the catalog that lists genres.
                let mut results = SearchResults::default();
                let mut error = None;
                let mut attempted = 0;
                for catalog in addons[0].manifest.catalogs.iter().filter(|c| c.id == "top") {
                    attempted += 1;
                    match client.catalog(&addons[0], catalog, &[("genre", term.as_str())]) {
                        Ok(items) => results.items.extend(items),
                        Err(failure) => {
                            results.failed += 1;
                            error = Some(failure);
                        }
                    }
                }
                match error {
                    Some(error) if results.failed == attempted => Err(error),
                    _ => Ok(results),
                }
            },
            cx,
        );
    }

    fn begin_results(
        &mut self,
        mode: SearchMode,
        work: impl FnOnce() -> Result<SearchResults, Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.search.generation += 1;
        let generation = self.search.generation;
        self.search.mode = mode;
        self.search.loading = true;
        self.search.error = None;
        self.search.failed = 0;
        self.cursor = None;
        self.main_scroll.set_offset(point(px(0.), px(0.)));
        on_thread(
            work,
            move |this, result, cx| {
                // A slower, older request must not replace a newer one.
                if generation != this.search.generation {
                    return;
                }
                this.search.loading = false;
                match result {
                    Ok(results) => {
                        this.prefetch_posters(&results.items);
                        if let Some(first) = results.items.first().cloned()
                            && this.tab == Tab::Search
                            && this.detail.is_none()
                        {
                            this.light_from(&first, cx);
                        }
                        this.search.items = results.items;
                        this.search.failed = results.failed;
                    }
                    Err(error) => {
                        this.search.items.clear();
                        this.search.error = Some(error.to_string());
                    }
                }
            },
            cx,
        );
        cx.notify();
    }

    pub fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.search.mode = SearchMode::Query(String::new());
        self.run_search(cx);
        window.focus(&self.search_input.focus_handle(cx), cx);
    }

    // ---- Collections ----------------------------------------------------------

    pub fn library_items(&self) -> Vec<Media> {
        self.profile
            .as_ref()
            .map(Profile::library_items)
            .unwrap_or_default()
    }

    pub fn continue_watching(&self) -> Vec<WatchProgress> {
        self.profile
            .as_ref()
            .map(Profile::continue_watching)
            .unwrap_or_default()
    }

    pub fn visible_items(&self) -> Vec<Media> {
        match self.tab {
            Tab::Search => self
                .search
                .items
                .iter()
                .filter(|m| self.search_filter.admits(m))
                .cloned()
                .collect(),
            Tab::Library => self
                .library_items()
                .into_iter()
                .filter(|m| self.library_filter.admits(m))
                .collect(),
            _ => vec![],
        }
    }

    pub fn saved(&self, media: &Media) -> bool {
        self.profile.as_ref().is_some_and(|p| p.contains(media))
    }

    pub fn toggle_save(&mut self, media: Media, cx: &mut Context<Self>) {
        let name = media.name.clone();
        let Some(profile) = &mut self.profile else {
            self.notice = Some(
                "Your library is unavailable. The file was left untouched; check the data folder."
                    .into(),
            );
            cx.notify();
            return;
        };
        match profile.toggle(media) {
            Ok(true) => self.announce(format!("Saved {name}"), cx),
            Ok(false) => self.announce(format!("Removed {name}"), cx),
            Err(error) => self.notice = Some(format!("Couldn’t update your library. {error}")),
        }
        cx.notify();
    }

    // ---- Details --------------------------------------------------------------

    pub fn open_title(&mut self, media: Media, cx: &mut Context<Self>) {
        self.prefetch_stage(&media);
        if let Some(poster) = &media.poster {
            self.want_art(Art::Original, poster);
        }
        self.light_from(&media, cx);
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
        self.detail = Some(DetailState {
            media: media.clone(),
            loading: true,
            error: None,
            season: None,
            video: None,
            streams: Load::Idle,
        });
        if !media.is_series() {
            self.select_video(media.id.clone(), cx);
        }
        let client = self.client.clone();
        let addons = self.all_addons();
        let key = media.key();
        on_thread(
            move || client.metadata(&media, &addons),
            move |this, result, cx| {
                let Some(detail) = this.detail.as_mut().filter(|d| d.media.key() == key) else {
                    return;
                };
                detail.loading = false;
                match result {
                    Ok(media) => {
                        let pending = this.pending_video.take();
                        detail.season = pending
                            .as_deref()
                            .and_then(season_of)
                            .filter(|s| media.seasons().contains(s))
                            .or_else(|| media.seasons().first().copied());
                        detail.media = media;
                        let media = detail.media.clone();
                        this.prefetch_stage(&media);
                        this.prefetch_season(cx);
                        if let Some(video) = pending.filter(|_| media.is_series()) {
                            this.select_video(video, cx);
                        }
                    }
                    Err(error) => detail.error = Some(error.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }

    pub fn select_season(&mut self, season: u32, cx: &mut Context<Self>) {
        if let Some(detail) = &mut self.detail {
            detail.season = Some(season);
            detail.video = None;
            detail.streams = Load::Idle;
        }
        self.prefetch_season(cx);
        cx.notify();
    }

    fn prefetch_season(&mut self, _cx: &mut Context<Self>) {
        let thumbnails: Vec<String> = self
            .detail
            .as_ref()
            .and_then(|d| d.season.map(|season| (d, season)))
            .map(|(d, season)| {
                d.media
                    .episodes(season)
                    .iter()
                    .filter_map(|e| e.thumbnail.clone())
                    .collect()
            })
            .unwrap_or_default();
        for thumbnail in thumbnails {
            self.want_art(Art::Original, &thumbnail);
        }
    }

    /// Choosing an episode reveals its sources; choosing it again hides them.
    pub fn select_video(&mut self, video: String, cx: &mut Context<Self>) {
        let addons = self.all_addons();
        let has_streams = addons.iter().any(|a| a.manifest.provides_streams());
        let Some(detail) = &mut self.detail else {
            return;
        };
        if detail.media.is_series() && detail.video.as_ref() == Some(&video) {
            detail.video = None;
            detail.streams = Load::Idle;
            cx.notify();
            return;
        }
        detail.video = Some(video.clone());
        if !has_streams {
            detail.streams = Load::Idle;
            cx.notify();
            return;
        }
        detail.streams = Load::Loading;
        let kind = detail.media.kind.clone();
        let client = self.client.clone();
        let expected = video.clone();
        on_thread(
            move || client.streams(&kind, &video, &addons),
            move |this, results, _| {
                if let Some(detail) = this
                    .detail
                    .as_mut()
                    .filter(|d| d.video.as_ref() == Some(&expected))
                {
                    detail.streams = Load::Ready(results);
                }
            },
            cx,
        );
        cx.notify();
    }

    // ---- Player -----------------------------------------------------------------

    /// The label for the Play button: where this video would resume, if anywhere.
    pub fn resume_point(&self, media: &Media, video_id: &str) -> Option<f64> {
        self.profile
            .as_ref()?
            .progress_for(video_id, media)
            .filter(WatchProgress::resumable)
            .map(|progress| progress.seconds)
    }

    /// Play on a title: whatever comes next for it, starting straight away.
    pub fn play_title(&mut self, media: Media, cx: &mut Context<Self>) {
        let latest = self.profile.as_ref().and_then(|p| p.latest_for(&media));
        let video = up_next(&media, latest.as_ref());
        self.play_video(media, video, cx);
    }

    /// A Continue Watching card: that video, from where it stopped.
    pub fn play_progress(&mut self, entry: &WatchProgress, cx: &mut Context<Self>) {
        self.play_video(entry.media.clone(), Some(entry.video_id.clone()), cx);
    }

    /// The one way into playback. The player opens immediately and finds its own
    /// source behind the title art: a download if there is one, or else the source
    /// used last, falling through to the next if one fails.
    pub fn play_video(&mut self, media: Media, video: Option<String>, cx: &mut Context<Self>) {
        if let Some(video) = &video
            && self.play_if_downloaded(&media, video, cx)
        {
            return;
        }
        let request = PlayRequest {
            episode_label: video.as_deref().and_then(|v| episode_label(&media, v)),
            media: media.clone(),
            video_id: video.clone().unwrap_or_default(),
            url: String::new(),
            filename: None,
            addon: String::new(),
            subtitles: vec![],
            force_compatibility: false,
            local: false,
        };
        self.begin_player(request, vec![], cx);
        let generation = self.player_generation;
        let latest = self.profile.as_ref().and_then(|p| p.latest_for(&media));
        let client = self.client.clone();
        let addons = self.all_addons();
        on_thread(
            move || {
                // Cards from the cloud know little: episodes come from the details.
                let thin = media.videos.is_empty() || media.description.is_none();
                let details = thin
                    .then(|| client.metadata(&media, &addons).ok())
                    .flatten();
                let media = details.unwrap_or(media);
                let video = video.or_else(|| up_next(&media, latest.as_ref()));
                let streams = video
                    .as_ref()
                    .map(|v| client.streams(&media.kind, v, &addons));
                (media, video, streams)
            },
            move |this, (media, video, streams), cx| {
                this.sources_found(generation, media, video, streams, cx);
            },
            cx,
        );
    }

    fn sources_found(
        &mut self,
        generation: u64,
        media: Media,
        video: Option<String>,
        streams: Option<StreamResults>,
        cx: &mut Context<Self>,
    ) {
        if self
            .player
            .as_ref()
            .is_none_or(|s| s.generation != generation)
        {
            return; // Closed, or something else started, while this was looking.
        }
        let last = self
            .profile
            .as_ref()
            .and_then(|p| p.last_source_for(&media))
            .cloned();
        let has_stream_addons = self.stream_addons_installed();
        let Some(state) = &mut self.player else {
            return;
        };
        let (Some(video), Some(streams)) = (video, streams) else {
            state.error = Some("This title has no episodes yet.".into());
            return;
        };
        state.request.episode_label = episode_label(&media, &video);
        state.request.media = media.clone();
        state.request.video_id = video.clone();
        let candidates = rank_sources(&streams, last.as_ref());
        if candidates.is_empty() {
            state.error = Some(if !has_stream_addons {
                "Add a stream add-on in Settings to play this.".into()
            } else if !streams.failures.is_empty() {
                "Your add-ons couldn’t be reached. Check your connection and try again.".into()
            } else {
                "No playable source was found for this video.".into()
            });
            return;
        }
        state.candidates = candidates;
        self.load_candidate(0, cx);
    }

    fn play_if_downloaded(&mut self, media: &Media, video: &str, cx: &mut Context<Self>) -> bool {
        let Some(downloads) = &self.downloads else {
            return false;
        };
        let Some(record) = downloads.find(media, video) else {
            return false;
        };
        let Some(path) = downloads.local_file(&record) else {
            return false;
        };
        let Ok(url) = url::Url::from_file_path(&path) else {
            return false;
        };
        let subtitles = downloads.subtitle_tracks(&record);
        let mut media = media.clone();
        // Keep any episode list we already have, for Next Episode.
        if media.description.is_none() {
            media = record.media.clone();
        }
        self.start_player(
            PlayRequest {
                episode_label: episode_label(&media, video).or(record.episode_label.clone()),
                media,
                video_id: video.to_owned(),
                url: url.to_string(),
                filename: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                addon: record.addon.clone(),
                subtitles,
                force_compatibility: false,
                local: true,
            },
            cx,
        );
        true
    }

    /// Playing from the sources list: that source first, with the rest behind it.
    pub fn play_source(&mut self, source: &StreamSource, cx: &mut Context<Self>) {
        let Some(detail) = &self.detail else {
            return;
        };
        let video_id = detail
            .video
            .clone()
            .unwrap_or_else(|| detail.media.id.clone());
        let media = detail.media.clone();
        let mut candidates = vec![source.clone()];
        if let Load::Ready(results) = &detail.streams {
            candidates.extend(
                rank_sources(results, None)
                    .into_iter()
                    .filter(|other| other != source),
            );
        }
        let request = PlayRequest {
            episode_label: episode_label(&media, &video_id),
            media,
            video_id,
            url: String::new(),
            filename: None,
            addon: String::new(),
            subtitles: vec![],
            force_compatibility: false,
            local: false,
        };
        self.begin_player(request, candidates, cx);
        self.load_candidate(0, cx);
    }

    /// Switching in the player's Source menu, carrying on from the same place.
    pub fn switch_source(&mut self, index: usize, cx: &mut Context<Self>) {
        self.save_progress();
        if let Some(state) = &mut self.player {
            state.menu = None;
        }
        self.load_candidate(index, cx);
    }

    fn load_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        let Some(source) = state.candidates.get(index).cloned() else {
            return;
        };
        let StreamAction::Direct(url) = source.stream.action() else {
            return;
        };
        state.candidate = index;
        state.remembered = false;
        state.request.url = url.to_owned();
        state.request.filename = source.stream.filename().map(str::to_owned);
        state.request.addon = source.addon.clone();
        state.request.force_compatibility = false;
        let known: HashSet<String> = state
            .addon_subtitles
            .iter()
            .map(|t| t.url.clone())
            .collect();
        state.addon_subtitles.extend(
            source
                .stream
                .subtitles
                .iter()
                .filter(|t| !known.contains(&t.url))
                .cloned(),
        );
        self.open_engine(cx);
    }

    /// Moves on when a source fails, and only reports failure when none are left.
    fn try_next_source(&mut self, failure: String, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        let next = state.candidate + 1;
        if next < state.candidates.len() {
            state.status = Some("Trying another source…".into());
            self.load_candidate(next, cx);
        } else {
            state.status = None;
            state.error = Some(failure);
            cx.notify();
        }
    }

    /// Back to the title with its sources showing, when none of them played.
    pub fn choose_source(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &self.player else {
            return;
        };
        let (media, video) = (state.request.media.clone(), state.request.video_id.clone());
        self.close_player(cx);
        if self
            .detail
            .as_ref()
            .is_none_or(|d| d.media.key() != media.key())
        {
            self.pending_video = Some(video).filter(|v| !v.is_empty());
            self.open_title(media, cx);
        } else if media.is_series()
            && !video.is_empty()
            && self
                .detail
                .as_ref()
                .is_some_and(|d| d.video.as_ref() != Some(&video))
        {
            self.select_video(video, cx);
        }
    }

    /// Debug builds: plays a URL directly, once the window has its native view.
    #[cfg(debug_assertions)]
    pub fn qa_play(&mut self, url: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            let _ = this.update(cx, |this, cx| {
                let media = this
                    .stage_titles()
                    .first()
                    .map(|m| (*m).clone())
                    .unwrap_or_default();
                // "title" goes through Play itself, source choice and all.
                if url == "title" {
                    this.play_title(media, cx);
                    return;
                }
                let video_id = media.id.clone();
                this.start_player(
                    PlayRequest {
                        media,
                        video_id,
                        episode_label: None,
                        url,
                        filename: None,
                        addon: "Local Test".into(),
                        local: false,
                        subtitles: std::env::var("STIMMY_QA_SUBTITLE")
                            .into_iter()
                            .map(|url| SubtitleTrack {
                                id: "qa".into(),
                                url,
                                lang: "eng".into(),
                            })
                            .collect(),
                        force_compatibility: false,
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    pub fn start_player(&mut self, request: PlayRequest, cx: &mut Context<Self>) {
        self.begin_player(request, vec![], cx);
        self.open_engine(cx);
    }

    /// Puts the player on screen. The title art holds it until a source is playing.
    fn begin_player(
        &mut self,
        request: PlayRequest,
        candidates: Vec<StreamSource>,
        cx: &mut Context<Self>,
    ) {
        self.close_player(cx);
        self.prefetch_stage(&request.media);
        self.light_from(&request.media, cx);
        self.player_generation += 1;
        let generation = self.player_generation;
        self.player = Some(PlayerState {
            addon_subtitles: request.subtitles.clone(),
            request,
            engine: None,
            snapshot: Snapshot::default(),
            error: None,
            cues: None,
            caption: None,
            controls_visible: true,
            last_activity: std::time::Instant::now(),
            scrub: None,
            hover_x: None,
            timeline_hovered: false,
            volume: self.saved_volume,
            muted: false,
            speed: 1.,
            menu: None,
            seek_target: None,
            last_saved: now(),
            generation,
            bezel: None,
            candidates,
            candidate: 0,
            opened_at: std::time::Instant::now(),
            status: None,
            remembered: false,
            subtitles_fetched: false,
            _awake: KeepAwake::begin(),
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if this
                        .player
                        .as_ref()
                        .is_some_and(|s| s.generation == generation)
                    {
                        this.player_tick(cx);
                        true
                    } else {
                        false
                    }
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Starts the current request on a fresh engine, from the saved position.
    fn open_engine(&mut self, cx: &mut Context<Self>) {
        let host = self.host_view;
        let Some(state) = &self.player else {
            return;
        };
        let start = self
            .resume_point(&state.request.media, &state.request.video_id)
            .unwrap_or(0.);
        let Some(state) = &mut self.player else {
            return;
        };
        if let Some(mut old) = state.engine.take() {
            old.shutdown();
        }
        state.snapshot = Snapshot::default();
        state.opened_at = std::time::Instant::now();
        let Some(host) = host else {
            state.error = Some("The video view isn’t ready.".into());
            return;
        };
        let opened = player::open(
            host,
            &state.request.url,
            state.request.filename.as_deref(),
            start,
            state.request.force_compatibility,
        );
        match opened {
            Ok(mut engine) => {
                engine.set_volume(if state.muted { 0. } else { state.volume });
                if state.speed != 1. {
                    engine.set_speed(state.speed);
                }
                state.engine = Some(engine);
                self.fetch_subtitles(cx);
            }
            Err(failure) => self.try_next_source(failure, cx),
        }
        cx.notify();
    }

    fn fetch_subtitles(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        if state.request.local || std::mem::replace(&mut state.subtitles_fetched, true) {
            return;
        }
        let generation = state.generation;
        let (kind, video) = (
            state.request.media.kind.clone(),
            state.request.video_id.clone(),
        );
        let client = self.client.clone();
        let addons = self.all_addons();
        on_thread(
            move || client.subtitles(&kind, &video, &addons),
            move |this, tracks, _| {
                if let Some(state) = this.player.as_mut().filter(|s| s.generation == generation) {
                    let known: HashSet<String> = state
                        .addon_subtitles
                        .iter()
                        .map(|t| t.url.clone())
                        .collect();
                    state
                        .addon_subtitles
                        .extend(tracks.into_iter().filter(|t| !known.contains(&t.url)));
                }
            },
            cx,
        );
    }

    fn player_tick(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        let Some(engine) = &mut state.engine else {
            cx.notify();
            return;
        };
        let snapshot = engine.snapshot();
        if state.error.is_none() {
            // A dead link can hang without ever failing, so it gets a time limit.
            let stalled = !snapshot.first_frame
                && !snapshot.paused
                && state.opened_at.elapsed() > Duration::from_secs(40);
            let failure = snapshot
                .failed
                .clone()
                .or_else(|| stalled.then(|| "This source isn’t responding.".to_owned()));
            if let Some(failure) = failure {
                // AVFoundation can't open everything its extension suggests it can.
                if engine.name() == "AVFoundation" && !stalled && player::mpv_available() {
                    state.request.force_compatibility = true;
                    self.open_engine(cx);
                } else {
                    self.try_next_source(failure, cx);
                }
                return;
            }
        }
        // Remembered only once it has really shown a picture.
        if snapshot.first_frame && !state.remembered {
            state.remembered = true;
            state.status = None;
            let source = state.candidates.get(state.candidate).map(LastSource::of);
            let media = state.request.media.clone();
            if let (Some(source), Some(profile)) = (source, &mut self.profile) {
                let _ = profile.remember_source(&media, source);
            }
        }
        let Some(state) = &mut self.player else {
            return;
        };
        if let Some(target) = state.seek_target
            && (snapshot.position - target).abs() < 4.
        {
            state.seek_target = None;
        }
        state.caption = state
            .cues
            .as_ref()
            .and_then(|loaded| cue_at(&loaded.cues, snapshot.position))
            .map(|cue| cue.text.clone());
        state.snapshot = snapshot;
        let paused = state.snapshot.paused || state.snapshot.ended;
        let busy = state.menu.is_some()
            || state.scrub.is_some()
            || state.error.is_some()
            || !state.snapshot.first_frame;
        if !paused
            && !busy
            && state.controls_visible
            && state.last_activity.elapsed() > Duration::from_millis(2600)
        {
            state.controls_visible = false;
            player::hide_cursor_until_moved();
            if let Some(host) = self.host_view {
                host.set_window_buttons_hidden(true);
            }
        }
        if now() - self.player.as_ref().map_or(0., |s| s.last_saved) >= 5. {
            self.save_progress();
        }
        cx.notify();
    }

    fn save_progress(&mut self) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.last_saved = now();
        if state.snapshot.position <= 0.
            || state.snapshot.duration <= 0.
            || state.request.video_id.is_empty()
        {
            return;
        }
        let entry = WatchProgress {
            media: state.request.media.clone(),
            video_id: state.request.video_id.clone(),
            episode_label: season_of(&state.request.video_id).map(|season| {
                let number = state.request.video_id.rsplit(':').next().unwrap_or("");
                format!("S{season} E{number}")
            }),
            seconds: state.snapshot.position,
            duration: state.snapshot.duration,
            updated_at: now(),
        };
        if let Some(profile) = &mut self.profile {
            let _ = profile.record(entry);
        }
    }

    pub fn close_player(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() {
            return;
        }
        self.save_progress();
        if let Some(mut state) = self.player.take() {
            self.saved_volume = state.volume;
            if let Some(engine) = &mut state.engine {
                engine.shutdown();
            }
        }
        if let Some(host) = self.host_view {
            host.set_window_buttons_hidden(false);
        }
        cx.notify();
    }

    pub fn player_activity(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.last_activity = std::time::Instant::now();
        if !state.controls_visible {
            state.controls_visible = true;
            if let Some(host) = self.host_view {
                host.set_window_buttons_hidden(false);
            }
            cx.notify();
        }
    }

    fn flash(&mut self, bezel: Bezel) {
        self.bezel_generation += 1;
        let generation = self.bezel_generation;
        if let Some(state) = &mut self.player {
            state.bezel = Some((generation, bezel));
        }
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.player_activity(cx);
        let Some(state) = &mut self.player else {
            return;
        };
        let Some(engine) = &mut state.engine else {
            return;
        };
        state.snapshot.paused = !state.snapshot.paused;
        engine.set_paused(state.snapshot.paused);
        let bezel = if state.snapshot.paused {
            Bezel::Pause
        } else {
            Bezel::Play
        };
        self.flash(bezel);
        cx.notify();
    }

    pub fn seek_to(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        let seconds = if state.snapshot.duration > 0. {
            seconds.clamp(0., state.snapshot.duration)
        } else {
            seconds.max(0.)
        };
        state.seek_target = Some(seconds);
        state.snapshot.ended = false;
        if let Some(engine) = &mut state.engine {
            engine.seek(seconds);
        }
        cx.notify();
    }

    pub fn seek_by(&mut self, delta: f64, cx: &mut Context<Self>) {
        self.player_activity(cx);
        let Some(state) = &self.player else {
            return;
        };
        let from = state.seek_target.unwrap_or(state.snapshot.position);
        self.seek_to(from + delta, cx);
        let label = if delta < 0. {
            format!("−{:.0}", -delta)
        } else {
            format!("+{delta:.0}")
        };
        self.flash(Bezel::Text(label));
    }

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.player_activity(cx);
        let Some(state) = &mut self.player else {
            return;
        };
        state.volume = volume.clamp(0., 1.);
        state.muted = false;
        if let Some(engine) = &mut state.engine {
            engine.set_volume(state.volume);
        }
        let label = format!("{:.0}%", state.volume * 100.);
        self.flash(Bezel::Text(label));
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.player_activity(cx);
        let Some(state) = &mut self.player else {
            return;
        };
        state.muted = !state.muted;
        let volume = if state.muted { 0. } else { state.volume };
        if let Some(engine) = &mut state.engine {
            engine.set_volume(volume);
        }
        let label = if state.muted {
            "Muted".to_owned()
        } else {
            format!("{:.0}%", volume * 100.)
        };
        self.flash(Bezel::Text(label));
        cx.notify();
    }

    pub fn set_speed(&mut self, speed: f32, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.speed = speed;
        state.menu = None;
        if let Some(engine) = &mut state.engine {
            engine.set_speed(speed);
        }
        cx.notify();
    }

    pub fn subtitles_off(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.cues = None;
        state.caption = None;
        state.menu = None;
        if let Some(engine) = &mut state.engine {
            engine.select_subtitle(None);
        }
        cx.notify();
    }

    pub fn select_engine_subtitle(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.cues = None;
        state.caption = None;
        state.menu = None;
        if let Some(engine) = &mut state.engine {
            engine.select_subtitle(Some(&id));
        }
        cx.notify();
    }

    pub fn select_audio(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.menu = None;
        if let Some(engine) = &mut state.engine {
            engine.select_audio(&id);
        }
        cx.notify();
    }

    /// Downloads an add-on's subtitle and draws it over either engine.
    pub fn select_addon_subtitle(&mut self, url: String, cx: &mut Context<Self>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.menu = None;
        if let Some(engine) = &mut state.engine {
            engine.select_subtitle(None);
        }
        let generation = state.generation;
        let client = self.client.clone();
        let fetch = url.clone();
        let downloads = self.downloads.clone();
        on_thread(
            move || {
                // Saved with a download, so this works offline.
                if fetch.starts_with("file:") {
                    downloads.ok_or(Error::Unreadable)?.local_cues(&fetch)
                } else {
                    client.subtitle_cues(&fetch)
                }
            },
            move |this, result, cx| match result {
                Ok(cues) => {
                    if let Some(state) = this.player.as_mut().filter(|s| s.generation == generation)
                    {
                        state.cues = Some(LoadedCues { url, cues });
                    }
                }
                Err(_) => this.announce("Those subtitles couldn’t load", cx),
            },
            cx,
        );
        cx.notify();
    }

    /// The next episode, if the current video is part of a series.
    pub fn next_video(&self) -> Option<String> {
        let state = self.player.as_ref()?;
        next_episode(&state.request.media, &state.request.video_id).map(|e| e.id.clone())
    }

    /// Straight into the next episode, on the same source if it has it.
    pub fn play_next(&mut self, cx: &mut Context<Self>) {
        let Some(video) = self.next_video() else {
            return;
        };
        let Some(media) = self.player.as_ref().map(|s| s.request.media.clone()) else {
            return;
        };
        self.play_video(media, Some(video), cx);
    }

    // ---- Downloads ------------------------------------------------------------

    /// Downloads a video, with `source` if one was chosen, or else the source Play
    /// would use. Saves its subtitles too, so they work offline.
    pub fn download_video(
        &mut self,
        media: Media,
        video: String,
        source: Option<StreamSource>,
        cx: &mut Context<Self>,
    ) {
        let key = format!("{}:{video}", media.key());
        if !self.preparing_downloads.insert(key.clone()) {
            return;
        }
        let last = self
            .profile
            .as_ref()
            .and_then(|p| p.last_source_for(&media))
            .cloned();
        let client = self.client.clone();
        let addons = self.all_addons();
        let (kind, id) = (media.kind.clone(), video.clone());
        on_thread(
            move || {
                let source = source.or_else(|| {
                    rank_sources(&client.streams(&kind, &id, &addons), last.as_ref())
                        .into_iter()
                        .next()
                });
                let tracks = source
                    .as_ref()
                    .map(|_| client.subtitles(&kind, &id, &addons));
                (source, tracks.unwrap_or_default())
            },
            move |this, (source, tracks), cx| {
                this.preparing_downloads.remove(&key);
                let Some(source) = source else {
                    this.announce(format!("No source found for {}", media.name), cx);
                    return;
                };
                let StreamAction::Direct(url) = source.stream.action() else {
                    return;
                };
                let Some(downloads) = &this.downloads else {
                    return;
                };
                let name = media.name.clone();
                let mut subtitles = source.stream.subtitles.clone();
                subtitles.extend(tracks);
                let added = downloads.add(NewDownload {
                    episode_label: episode_label(&media, &video),
                    media,
                    video_id: video,
                    addon: source.addon.clone(),
                    stream_name: source.stream.name.clone(),
                    binge_group: source.stream.binge_group().map(str::to_owned),
                    url: url.to_owned(),
                    filename_hint: source.stream.filename().map(str::to_owned),
                    subtitles,
                });
                match added {
                    Ok(_) => {
                        this.announce(format!("Downloading {name}"), cx);
                        this.ensure_download_ticker(cx);
                    }
                    Err(error) => this.announce(error.to_string(), cx),
                }
            },
            cx,
        );
        cx.notify();
    }

    pub fn download_season(&mut self, media: Media, season: u32, cx: &mut Context<Self>) {
        let episodes: Vec<String> = media
            .episodes(season)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        for video in episodes {
            let already = self.downloads.as_ref().is_some_and(|d| {
                d.find(&media, &video)
                    .is_some_and(|r| r.state != stimmy_core::DownloadState::Failed)
            });
            if !already {
                self.download_video(media.clone(), video, None, cx);
            }
        }
    }

    /// Polls only while something is downloading, and stops when nothing is.
    pub fn ensure_download_ticker(&mut self, cx: &mut Context<Self>) {
        if self.downloads_ticking || !self.downloads.as_ref().is_some_and(Downloads::any_active) {
            return;
        }
        self.downloads_ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let keep_going = this.update(cx, |this, cx| this.download_tick(cx));
                if !matches!(keep_going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn download_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(downloads) = self.downloads.clone() else {
            self.downloads_ticking = false;
            return false;
        };
        for event in downloads.take_events() {
            match event {
                DownloadEvent::Completed { title, .. } => {
                    self.announce(format!("Downloaded {title}"), cx)
                }
                DownloadEvent::Failed { title, .. } => {
                    self.announce(format!("{title} couldn’t finish downloading"), cx)
                }
                DownloadEvent::LinkExpired { id } => self.refresh_download_link(id, cx),
            }
        }
        cx.notify();
        let active = downloads.any_active();
        self.downloads_ticking = active;
        active
    }

    /// Add-on links expire, so an expired one asks the add-on again. The partial
    /// file is kept if the same source comes back.
    fn refresh_download_link(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(downloads) = self.downloads.clone() else {
            return;
        };
        let Some(record) = downloads.records().into_iter().find(|r| r.id == id) else {
            return;
        };
        let attempts = self.link_refreshes.entry(id.clone()).or_insert(0);
        *attempts += 1;
        if *attempts > 2 {
            downloads.fail_expired(&id);
            cx.notify();
            return;
        }
        let client = self.client.clone();
        let addons = self.all_addons();
        let last = LastSource {
            addon: record.addon.clone(),
            name: record.stream_name.clone(),
            binge_group: record.binge_group.clone(),
        };
        let (kind, video) = (record.media.kind.clone(), record.video_id.clone());
        on_thread(
            move || {
                rank_sources(&client.streams(&kind, &video, &addons), Some(&last))
                    .into_iter()
                    .next()
            },
            move |this, source: Option<StreamSource>, cx| {
                let Some(downloads) = this.downloads.clone() else {
                    return;
                };
                match source.as_ref().map(|s| (s, s.stream.action())) {
                    Some((source, StreamAction::Direct(url))) => {
                        let same = source.addon == record.addon
                            && source.stream.name == record.stream_name;
                        downloads.refresh_link(&id, url, same);
                        this.ensure_download_ticker(cx);
                    }
                    _ => downloads.fail_expired(&id),
                }
            },
            cx,
        );
    }

    pub fn pause_download(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(downloads) = &self.downloads {
            downloads.pause(id);
        }
        cx.notify();
    }

    /// Resuming or retrying. An expired link gets a fresh one first.
    pub fn resume_download(&mut self, record: &DownloadRecord, cx: &mut Context<Self>) {
        let Some(downloads) = self.downloads.clone() else {
            return;
        };
        if record.link_expired {
            self.link_refreshes.remove(&record.id);
            self.refresh_download_link(record.id.clone(), cx);
        } else {
            downloads.resume(&record.id);
            self.ensure_download_ticker(cx);
        }
        cx.notify();
    }

    pub fn remove_download(&mut self, id: String, cx: &mut Context<Self>) {
        if self.removing_download.as_ref() != Some(&id) {
            self.removing_download = Some(id);
        } else {
            self.removing_download = None;
            if let Some(downloads) = &self.downloads {
                downloads.remove(&id);
            }
        }
        cx.notify();
    }

    pub fn play_download(&mut self, record: &DownloadRecord, cx: &mut Context<Self>) {
        self.play_if_downloaded(&record.media, &record.video_id, cx);
    }

    // ---- Accounts --------------------------------------------------------------

    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        let email = self.email_input.read(cx).value().trim().to_owned();
        let password = self.password_input.read(cx).value().to_string();
        if email.is_empty() || password.is_empty() || self.signing_in {
            return;
        }
        self.signing_in = true;
        self.account_error = None;
        let client = self.client.clone();
        on_thread(
            move || client.login(&email, &password),
            |this, result, cx| {
                this.signing_in = false;
                match result {
                    Ok(session) => {
                        let id = session.user.id.clone();
                        let mut next = this.accounts.clone();
                        next.retain(|a| a.user.id != id);
                        next.push(session);
                        // A Keychain failure shouldn't undo a good sign-in: the
                        // account works until quit, and the page says why it won't
                        // be remembered. A session is never written to a plain file.
                        let saved = this.save_sessions(&next);
                        this.accounts = next;
                        this.clear_credentials = true;
                        this.switch_account(Some(id), cx);
                        if let Err(error) = saved {
                            this.account_error = Some(format!(
                                "Signed in for now, but you’ll need to sign in again next time. {error}"
                            ));
                        }
                    }
                    Err(error) => this.account_error = Some(error.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }

    pub fn switch_account(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.close_player(cx);
        self.detail = None;
        self.active_account = id;
        write_active_account(&self.data_dir, self.active_account.as_deref());
        match Profile::open(profile_dir(&self.data_dir, self.active_account.as_deref())) {
            Ok(profile) => self.profile = Some(profile),
            Err(error) => {
                self.profile = None;
                self.notice = Some(format!(
                    "That profile couldn’t be opened and is untouched. {error}"
                ));
            }
        }
        self.prefetch_library(cx);
        self.load_shelves(cx);
        self.sync_account(cx);
        cx.notify();
    }

    pub fn remove_account(&mut self, id: String, cx: &mut Context<Self>) {
        let mut next = self.accounts.clone();
        next.retain(|a| a.user.id != id);
        let saved = self.save_sessions(&next);
        self.accounts = next;
        if self.active_account.as_ref() == Some(&id) {
            self.switch_account(None, cx);
        }
        if let Err(error) = saved {
            self.account_error = Some(format!(
                "Signed out for now, but it may return next launch. {error}"
            ));
        }
        cx.notify();
    }

    fn save_sessions(&self, sessions: &[AccountSession]) -> Result<(), Error> {
        let Some(service) = &self.keychain_service else {
            return Ok(());
        };
        keychain::write(service, sessions)?;
        let marker = self.data_dir.join("accounts-known");
        if sessions.is_empty() {
            let _ = std::fs::remove_file(marker);
        } else {
            stimmy_core::store::atomic_write(&marker, b"", false)?;
        }
        Ok(())
    }

    /// Reads add-ons and library from the account. Failures keep what's saved.
    pub fn sync_account(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.active_session().cloned() else {
            return;
        };
        self.syncing = true;
        self.sync_generation += 1;
        let generation = self.sync_generation;
        let account = session.user.id.clone();
        let client = self.client.clone();
        on_thread(
            move || {
                (
                    client.account_addons(&session.auth_key),
                    client.cloud_library(&session.auth_key),
                )
            },
            move |this, (addons, library), cx| {
                if generation != this.sync_generation
                    || this.active_account.as_ref() != Some(&account)
                {
                    return;
                }
                this.syncing = false;
                this.account_error = None;
                let mut reload = false;
                if let Some(profile) = &mut this.profile {
                    match addons {
                        Ok(addons) => {
                            reload = addons != profile.account_addons();
                            let _ = profile.set_account_addons(addons);
                        }
                        Err(error) => {
                            this.account_error = Some(format!("Couldn’t sync add-ons. {error}"))
                        }
                    }
                    match library {
                        Ok(items) => {
                            let _ = profile.set_cloud(items);
                        }
                        Err(error) => {
                            this.account_error =
                                Some(format!("Couldn’t sync your library. {error}"))
                        }
                    }
                }
                this.prefetch_library(cx);
                if reload {
                    this.load_shelves(cx);
                }
            },
            cx,
        );
        cx.notify();
    }

    // ---- Settings -------------------------------------------------------------

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(detail) = &self.detail {
            let media = detail.media.clone();
            self.open_title(media, cx);
            return;
        }
        match self.tab {
            Tab::Search => {
                let mode = std::mem::take(&mut self.search.mode);
                match mode {
                    SearchMode::Genre(genre) => self.browse_genre(genre, cx),
                    _ => self.run_search(cx),
                }
            }
            _ => self.load_shelves(cx),
        }
        // A failed image may have been temporary; a later view can ask again.
        let failed: Vec<_> = self.art_failed.drain().collect();
        for key in failed {
            self.art_requested.remove(&key);
        }
    }

    pub fn install_addon(&mut self, cx: &mut Context<Self>) {
        let input = self.addon_input.read(cx).value().trim().to_owned();
        if input.is_empty() || self.installing_addon {
            return;
        }
        self.installing_addon = true;
        self.addon_error = None;
        let client = self.client.clone();
        on_thread(
            move || client.install(&input),
            |this, result, cx| {
                this.installing_addon = false;
                match result {
                    Ok(addon) => {
                        let name = addon.manifest.name.clone();
                        let saved = match &mut this.addons {
                            Some(store) => store.install(addon),
                            None => Err(Error::Storage("the add-on list is unavailable".into())),
                        };
                        match saved {
                            Ok(()) => {
                                this.clear_addon_input = true;
                                this.announce(format!("Added {name}"), cx);
                                this.load_shelves(cx);
                            }
                            Err(error) => this.addon_error = Some(error.to_string()),
                        }
                    }
                    Err(error) => this.addon_error = Some(error.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }

    pub fn remove_addon(&mut self, url: String, cx: &mut Context<Self>) {
        if self.removing_addon.as_ref() != Some(&url) {
            self.removing_addon = Some(url);
            cx.notify();
            return;
        }
        self.removing_addon = None;
        if let Some(store) = &mut self.addons {
            match store.remove(&url) {
                Ok(()) => self.load_shelves(cx),
                Err(error) => self.addon_error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    pub fn measure_cache(&mut self, cx: &mut Context<Self>) {
        let artwork = self.artwork.clone();
        on_thread(
            move || artwork.size(),
            |this, size, _| this.cache_bytes = Some(size),
            cx,
        );
    }

    pub fn clear_cache(&mut self, cx: &mut Context<Self>) {
        let artwork = self.artwork.clone();
        let catalogs = CatalogCache::new(self.catalog_cache_dir.clone());
        on_thread(
            move || {
                let _ = catalogs.clear();
                artwork.clear()
            },
            |this, result, cx| {
                this.art.clear();
                this.art_requested.clear();
                this.art_failed.clear();
                this.light = None;
                this.previous_light = None;
                match result {
                    Ok(()) => this.announce("Cache cleared", cx),
                    Err(error) => this.notice = Some(error.to_string()),
                }
                this.measure_cache(cx);
                this.load_shelves(cx);
            },
            cx,
        );
    }

    // ---- Island ---------------------------------------------------------------

    /// A brief status in the navigation pill: it widens around the message,
    /// then settles back. Quiet feedback with nothing to dismiss.
    pub fn announce(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.island = Some(message.into());
        self.island_generation += 1;
        let generation = self.island_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1900))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.island_generation == generation {
                    this.island = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // ---- Keyboard -------------------------------------------------------------

    pub fn move_cursor(&mut self, key: &str, columns: usize, cx: &mut Context<Self>) -> bool {
        if self.detail.is_some() {
            return false;
        }
        if self.tab == Tab::Home {
            let rows: Vec<usize> = self.shelves.iter().map(|s| s.items.len()).collect();
            if rows.iter().all(|count| *count == 0) {
                return false;
            }
            let start = rows.iter().position(|c| *c > 0).unwrap_or(0);
            let Some(cursor) = self.cursor else {
                if matches!(key, "down" | "right") {
                    self.cursor = Some(Cursor { row: start, col: 0 });
                    self.cursor_moved(cx);
                    return true;
                }
                return false;
            };
            let Cursor { mut row, mut col } = cursor;
            match key {
                "left" => col = col.saturating_sub(1),
                "right" => col = (col + 1).min(rows[row].saturating_sub(1)),
                "up" => {
                    if let Some(previous) = (0..row).rev().find(|r| rows[*r] > 0) {
                        row = previous;
                        col = col.min(rows[row] - 1);
                    } else {
                        self.cursor = None;
                        self.main_scroll.set_offset(point(px(0.), px(0.)));
                        self.stage_changed(cx);
                        cx.notify();
                        return true;
                    }
                }
                "down" => {
                    if let Some(next) = (row + 1..rows.len()).find(|r| rows[*r] > 0) {
                        row = next;
                        col = col.min(rows[row] - 1);
                    }
                }
                "enter" => {
                    let media = self.shelves[row].items[col].clone();
                    self.open_title(media, cx);
                    return true;
                }
                _ => return false,
            }
            self.cursor = Some(Cursor { row, col });
            self.cursor_moved(cx);
            return true;
        }
        let count = self.visible_items().len();
        if count == 0 || !matches!(self.tab, Tab::Search | Tab::Library) {
            return false;
        }
        let Some(cursor) = self.cursor else {
            if matches!(key, "down" | "right") {
                self.cursor = Some(Cursor { row: 0, col: 0 });
                self.cursor_moved(cx);
                return true;
            }
            return false;
        };
        let col = match key {
            "left" => cursor.col.saturating_sub(1),
            "right" => (cursor.col + 1).min(count - 1),
            "up" => cursor.col.saturating_sub(columns),
            "down" => (cursor.col + columns).min(count - 1),
            "enter" => {
                let media = self.visible_items().remove(cursor.col);
                self.open_title(media, cx);
                return true;
            }
            _ => return false,
        };
        self.cursor = Some(Cursor { row: 0, col });
        self.cursor_moved(cx);
        true
    }

    fn cursor_moved(&mut self, cx: &mut Context<Self>) {
        let Some(Cursor { row, col }) = self.cursor else {
            return;
        };
        let media = if self.tab == Tab::Home {
            let state = &self.shelves[row];
            state.scroll.scroll_to_item(col);
            // The stage is child 0 of the page, then any Continue Watching shelf.
            let resume = usize::from(!self.continue_watching().is_empty());
            self.main_scroll.scroll_to_item(row + 1 + resume);
            state.items[col].clone()
        } else {
            self.visible_items().remove(col)
        };
        if !cx.reduce_motion() {
            self.light_from(&media, cx);
        }
        cx.notify();
    }
}
