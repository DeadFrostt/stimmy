//! libmpv, rendered in software and drawn by GPUI.
//!
//! The library is opened at runtime, so the app runs without it and simply says
//! so if a video needs it. Frames come out of mpv's software render API as BGRA
//! and are handed to the player, which draws them as an image. A dedicated
//! thread renders, because a 1080p frame takes long enough that doing it on the
//! interface thread would stutter the controls.

use super::super::{Engine, Frame, Snapshot, Track};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    thread::JoinHandle,
};

type Handle = *mut c_void;
type RenderContext = *mut c_void;

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventEndFile {
    reason: c_int,
    error: c_int,
}

const PARAM_INVALID: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_SW_SIZE: c_int = 17;
const PARAM_SW_FORMAT: c_int = 18;
const PARAM_SW_STRIDE: c_int = 19;
const PARAM_SW_POINTER: c_int = 20;
const EVENT_END_FILE: c_int = 7;
const END_FILE_REASON_ERROR: c_int = 4;
const RENDER_UPDATE_FRAME: u64 = 1;
/// mpv asks for a target aligned to this, and its scaler uses aligned SIMD.
const ALIGNMENT: usize = 64;

/// The functions Stimmy uses, resolved once from whichever libmpv was found.
struct Api {
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(Handle),
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    command: unsafe extern "C" fn(Handle, *mut *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut MpvEvent,
    render_create: unsafe extern "C" fn(*mut RenderContext, Handle, *mut RenderParam) -> c_int,
    render_update_callback:
        unsafe extern "C" fn(RenderContext, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render_update: unsafe extern "C" fn(RenderContext) -> u64,
    render_render: unsafe extern "C" fn(RenderContext, *mut RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(RenderContext),
    _library: Library,
}

fn load() -> Result<Api, String> {
    let candidates = [
        std::env::var("STIMMY_LIBMPV").unwrap_or_default(),
        // The sonames distributions ship, newest first.
        "libmpv.so.2".to_owned(),
        "libmpv.so.1".to_owned(),
        "libmpv.so".to_owned(),
    ];
    let mut failure = String::new();
    for path in candidates.iter().filter(|p| !p.is_empty()) {
        let library = match unsafe { Library::new(path) } {
            Ok(library) => library,
            Err(error) => {
                failure = error.to_string();
                continue;
            }
        };
        macro_rules! symbol {
            ($name:literal) => {
                *unsafe { library.get($name) }.map_err(|e| e.to_string())?
            };
        }
        return Ok(Api {
            create: symbol!(b"mpv_create\0"),
            initialize: symbol!(b"mpv_initialize\0"),
            terminate_destroy: symbol!(b"mpv_terminate_destroy\0"),
            set_option_string: symbol!(b"mpv_set_option_string\0"),
            set_property_string: symbol!(b"mpv_set_property_string\0"),
            get_property_string: symbol!(b"mpv_get_property_string\0"),
            free: symbol!(b"mpv_free\0"),
            command: symbol!(b"mpv_command\0"),
            wait_event: symbol!(b"mpv_wait_event\0"),
            render_create: symbol!(b"mpv_render_context_create\0"),
            render_update_callback: symbol!(b"mpv_render_context_set_update_callback\0"),
            render_update: symbol!(b"mpv_render_context_update\0"),
            render_render: symbol!(b"mpv_render_context_render\0"),
            render_free: symbol!(b"mpv_render_context_free\0"),
            _library: library,
        });
    }
    Err(failure)
}

fn api() -> Result<&'static Api, &'static str> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(|_| {
        "This video needs the libmpv engine, which isn’t installed. Install mpv with your package manager, then try again."
    })
}

pub fn available() -> bool {
    api().is_ok()
}

/// Shared with the render thread.
struct Shared {
    handle: Handle,
    render: Mutex<RenderContext>,
    /// The newest picture, waiting for the player to draw it.
    frame: Mutex<Option<Frame>>,
    /// mpv's scaler wants a 64-byte aligned target and a `Vec<u8>` isn't one,
    /// so frames are drawn into this and copied out.
    scratch: Mutex<Vec<u8>>,
    /// The size the picture is drawn at, in device pixels.
    viewport: Mutex<(u32, u32)>,
    /// The size mpv renders at: the video, shrunk to fit the viewport.
    target: Mutex<(i32, i32)>,
    /// The video's own size, as mpv reports it.
    video: (AtomicI32, AtomicI32),
    first_frame: AtomicBool,
    closed: AtomicBool,
    /// Wakes the render thread when mpv has a new picture.
    signal: async_channel::Sender<()>,
    /// Tells the player a picture is ready to draw.
    wake: async_channel::Sender<()>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    /// The video, shrunk to fit the viewport without changing its shape. Never
    /// enlarged: GPUI scales the picture up on the GPU for free.
    fn refresh_target(&self) {
        let (video_width, video_height) = (
            self.video.0.load(Ordering::Relaxed) as f32,
            self.video.1.load(Ordering::Relaxed) as f32,
        );
        let (viewport_width, viewport_height) = *self.viewport.lock().unwrap();
        let (viewport_width, viewport_height) = (viewport_width as f32, viewport_height as f32);
        if video_width < 1. || video_height < 1. {
            return;
        }
        let scale = if viewport_width < 1. || viewport_height < 1. {
            1.
        } else {
            (viewport_width / video_width)
                .min(viewport_height / video_height)
                .min(1.)
        };
        let target = (
            (video_width * scale).round().max(2.) as i32,
            (video_height * scale).round().max(2.) as i32,
        );
        *self.target.lock().unwrap() = target;
    }
}

unsafe extern "C" fn on_update(shared: *mut c_void) {
    // Called from mpv's threads, so nothing here may call back into mpv.
    let shared = unsafe { &*(shared as *const Shared) };
    let _ = shared.signal.try_send(());
}

/// Renders one picture into a fresh buffer. Returns whether it is worth drawing.
fn render_once(shared: &Shared) -> bool {
    let Ok(api) = api() else {
        return false;
    };
    let Ok(render) = shared.render.lock() else {
        return false;
    };
    if render.is_null() {
        return false;
    }
    let (width, height) = *shared.target.lock().unwrap();
    if width < 2 || height < 2 {
        return false;
    }
    // mpv asks for a 64-byte aligned stride as well as a 64-byte aligned
    // pointer; anything less makes its scaler copy the whole frame.
    let row = width as usize * 4;
    let stride = row.div_ceil(ALIGNMENT) * ALIGNMENT;
    let length = stride * height as usize;
    let Ok(mut scratch) = shared.scratch.lock() else {
        return false;
    };
    if scratch.len() < length + ALIGNMENT {
        scratch.resize(length + ALIGNMENT, 0);
    }
    // mpv's scaler reads and writes the target with aligned SIMD, and says an
    // unaligned pointer can crash.
    let base = scratch.as_mut_ptr();
    let offset = (ALIGNMENT - (base as usize % ALIGNMENT)) % ALIGNMENT;
    let target = unsafe { base.add(offset) };
    let mut size = [width, height];
    let format = cstring("bgr0");
    let mut stride_value = stride;
    let mut params = [
        RenderParam {
            kind: PARAM_SW_SIZE,
            data: size.as_mut_ptr() as *mut c_void,
        },
        RenderParam {
            kind: PARAM_SW_FORMAT,
            data: format.as_ptr() as *mut c_void,
        },
        RenderParam {
            kind: PARAM_SW_STRIDE,
            data: &mut stride_value as *mut _ as *mut c_void,
        },
        RenderParam {
            kind: PARAM_SW_POINTER,
            // mpv takes the buffer itself here, not a pointer to it.
            data: target as *mut c_void,
        },
        RenderParam {
            kind: PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];
    let flags = unsafe { (api.render_update)(*render) };
    // mpv only has a picture to give once it says so; rendering before that
    // hands its scaler an empty frame.
    if flags & RENDER_UPDATE_FRAME == 0 {
        return false;
    }
    if unsafe { (api.render_render)(*render, params.as_mut_ptr()) } < 0 {
        return false;
    }
    // The stride is padded, so the rows are packed as they are copied out.
    let source = unsafe { std::slice::from_raw_parts(target, length) };
    let mut pixels = Vec::with_capacity(row * height as usize);
    for line in source.chunks_exact(stride).take(height as usize) {
        pixels.extend_from_slice(&line[..row]);
    }
    // bgr0 leaves the fourth byte unused, and GPUI blends on it.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    shared.first_frame.store(true, Ordering::Release);
    *shared.frame.lock().unwrap() = Some(Frame {
        width: width as u32,
        height: height as u32,
        bgra: pixels,
    });
    true
}

fn cstring(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

pub struct MpvEngine {
    api: &'static Api,
    shared: *const Shared,
    signals: async_channel::Sender<()>,
    wake: async_channel::Receiver<()>,
    renderer: Option<JoinHandle<()>>,
    failed: Option<String>,
    closed: bool,
}

impl MpvEngine {
    pub fn open(url: &str, start: f64) -> Result<Self, &'static str> {
        let api = api()?;
        unsafe {
            let handle = (api.create)();
            if handle.is_null() {
                return Err("The libmpv engine couldn’t start.");
            }
            let option = |name: &str, value: &str| {
                let (name, value) = (cstring(name), cstring(value));
                (api.set_option_string)(handle, name.as_ptr(), value.as_ptr());
            };
            for (name, value) in [
                ("vo", "libmpv"),
                ("config", "no"),
                ("load-scripts", "no"),
                // Never hand a URL to an external downloader.
                ("ytdl", "no"),
                ("terminal", "no"),
                ("idle", "yes"),
                ("keep-open", "yes"),
                ("input-default-bindings", "no"),
                ("input-vo-keyboard", "no"),
                ("osc", "no"),
                // The software render API can't take hardware surfaces.
                ("hwdec", "no"),
                ("cache", "yes"),
                ("demuxer-max-bytes", "200MiB"),
                ("network-timeout", "20"),
                ("user-agent", concat!("Stimmy/", env!("CARGO_PKG_VERSION"))),
                ("audio-client-name", "Stimmy"),
                ("sub-auto", "no"),
            ] {
                option(name, value);
            }
            if start > 0. {
                option("start", &format!("{start:.3}"));
            }
            if (api.initialize)(handle) < 0 {
                (api.terminate_destroy)(handle);
                return Err("The libmpv engine couldn’t start.");
            }
            let (signal, signals) = async_channel::bounded(1);
            let (wake, wake_rx) = async_channel::bounded(1);
            let shared = Box::into_raw(Box::new(Shared {
                handle,
                render: Mutex::new(ptr::null_mut()),
                frame: Mutex::new(None),
                scratch: Mutex::new(Vec::new()),
                viewport: Mutex::new((0, 0)),
                target: Mutex::new((0, 0)),
                video: (AtomicI32::new(0), AtomicI32::new(0)),
                first_frame: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                signal: signal.clone(),
                wake,
            }));
            let mut render: RenderContext = ptr::null_mut();
            let mut params = [
                RenderParam {
                    kind: PARAM_API_TYPE,
                    data: c"sw".as_ptr() as *mut c_void,
                },
                RenderParam {
                    kind: PARAM_INVALID,
                    data: ptr::null_mut(),
                },
            ];
            if (api.render_create)(&mut render, handle, params.as_mut_ptr()) < 0 {
                (api.terminate_destroy)(handle);
                drop(Box::from_raw(shared));
                return Err("The libmpv engine couldn’t start.");
            }
            *(*shared).render.lock().unwrap() = render;
            (api.render_update_callback)(render, Some(on_update), shared as *mut c_void);
            // The render thread owns the shared state for as long as it runs.
            let renderer_shared = shared as usize;
            let renderer = std::thread::Builder::new()
                .name("stimmy-video".into())
                .spawn(move || {
                    let shared = renderer_shared as *const Shared;
                    while signals.recv_blocking().is_ok() {
                        let shared = &*shared;
                        if shared.closed.load(Ordering::Acquire) {
                            break;
                        }
                        if render_once(shared) {
                            let _ = shared.wake.try_send(());
                        }
                    }
                })
                .map_err(|_| "The libmpv engine couldn’t start.")?;
            let mut engine = Self {
                api,
                shared,
                signals: signal,
                wake: wake_rx,
                renderer: Some(renderer),
                failed: None,
                closed: false,
            };
            engine.load(url);
            Ok(engine)
        }
    }

    fn load(&mut self, url: &str) {
        let url = cstring(url);
        let (load, replace) = (cstring("loadfile"), cstring("replace"));
        let mut command = [load.as_ptr(), url.as_ptr(), replace.as_ptr(), ptr::null()];
        if unsafe { (self.api.command)(self.shared().handle, command.as_mut_ptr()) } < 0 {
            self.failed = Some("This address can’t be played.".into());
        }
    }

    fn shared(&self) -> &Shared {
        unsafe { &*self.shared }
    }
    fn property(&self, name: &str) -> Option<String> {
        let name = cstring(name);
        unsafe {
            let value = (self.api.get_property_string)(self.shared().handle, name.as_ptr());
            if value.is_null() {
                return None;
            }
            let text = CStr::from_ptr(value).to_string_lossy().into_owned();
            (self.api.free)(value as *mut c_void);
            Some(text)
        }
    }
    fn number(&self, name: &str) -> f64 {
        self.property(name)
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite())
            .unwrap_or(0.)
    }
    fn flag(&self, name: &str) -> bool {
        self.property(name).as_deref() == Some("yes")
    }
    fn set(&self, name: &str, value: &str) {
        let (name, value) = (cstring(name), cstring(value));
        unsafe {
            (self.api.set_property_string)(self.shared().handle, name.as_ptr(), value.as_ptr());
        }
    }
    fn drain_events(&mut self) {
        unsafe {
            loop {
                let event = (self.api.wait_event)(self.shared().handle, 0.);
                if event.is_null() || (*event).event_id == 0 {
                    break;
                }
                if (*event).event_id == EVENT_END_FILE && !(*event).data.is_null() {
                    let end = &*((*event).data as *const MpvEventEndFile);
                    if end.reason == END_FILE_REASON_ERROR {
                        self.failed = Some("This video couldn’t be opened.".into());
                    }
                }
            }
        }
    }
    fn tracks(&self) -> (Vec<Track>, Vec<Track>) {
        let (mut audio, mut subtitles) = (vec![], vec![]);
        let list = self.property("track-list").unwrap_or_default();
        for track in track_list::tracks(&list) {
            let parts: Vec<&str> = [track.title.as_deref(), track.lang.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            let name = match parts.as_slice() {
                [title, lang] => format!("{title} ({})", stimmy_core::language_name(lang)),
                [only] if track.lang.is_some() => stimmy_core::language_name(only),
                [only] => (*only).to_owned(),
                _ => format!("Track {}", track.id),
            };
            let entry = Track {
                id: track.id.to_string(),
                name,
                selected: track.selected,
            };
            match track.kind.as_str() {
                "audio" => audio.push(entry),
                "sub" => subtitles.push(entry),
                _ => {}
            }
        }
        (audio, subtitles)
    }
}

impl Engine for MpvEngine {
    fn snapshot(&mut self) -> Snapshot {
        self.drain_events();
        let (audio, subtitles) = self.tracks();
        let position = self.number("time-pos");
        let shared = self.shared();
        let (width, height) = (self.number("dwidth"), self.number("dheight"));
        if width > 0. && height > 0. {
            shared.video.0.store(width as i32, Ordering::Relaxed);
            shared.video.1.store(height as i32, Ordering::Relaxed);
            shared.refresh_target();
        }
        let first_frame = shared.first_frame.load(Ordering::Acquire);
        Snapshot {
            position,
            duration: self.number("duration"),
            buffered: position + self.number("demuxer-cache-duration"),
            paused: self.flag("pause"),
            buffering: self.flag("paused-for-cache") || self.flag("seeking") || !first_frame,
            ended: self.flag("eof-reached"),
            first_frame,
            failed: self.failed.clone(),
            audio,
            subtitles,
        }
    }
    fn set_paused(&mut self, paused: bool) {
        self.set("pause", if paused { "yes" } else { "no" });
    }
    fn seek(&mut self, target: f64) {
        let target = target.max(0.).to_string();
        let (seek, target, mode) = (
            cstring("seek"),
            cstring(&target),
            cstring("absolute+keyframes"),
        );
        let mut command = [seek.as_ptr(), target.as_ptr(), mode.as_ptr(), ptr::null()];
        unsafe {
            (self.api.command)(self.shared().handle, command.as_mut_ptr());
        }
    }
    fn set_volume(&mut self, volume: f32) {
        self.set(
            "volume",
            &format!("{}", (volume.clamp(0., 1.) * 100.).round()),
        );
    }
    fn set_speed(&mut self, speed: f32) {
        self.set("speed", &speed.to_string());
    }
    fn select_audio(&mut self, id: &str) {
        self.set("aid", id);
    }
    fn select_subtitle(&mut self, id: Option<&str>) {
        self.set("sid", id.unwrap_or("no"));
    }
    fn take_frame(&mut self) -> Option<Frame> {
        self.shared()
            .frame
            .lock()
            .ok()
            .and_then(|mut frame| frame.take())
    }
    fn frame_ready(&self) -> Option<async_channel::Receiver<()>> {
        Some(self.wake.clone())
    }
    fn set_viewport(&mut self, width: u32, height: u32) {
        let shared = self.shared();
        *shared.viewport.lock().unwrap() = (width, height);
        shared.refresh_target();
    }
    fn shutdown(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        let renderer = self.renderer.take();
        let shared = self.shared();
        // Stop the render thread before releasing anything it is using.
        shared.closed.store(true, Ordering::Release);
        let _ = self.signals.try_send(());
        if let Some(renderer) = renderer {
            let _ = renderer.join();
        }
        unsafe {
            if let Ok(mut render) = shared.render.lock()
                && !render.is_null()
            {
                (self.api.render_update_callback)(*render, None, ptr::null_mut());
                (self.api.render_free)(*render);
                *render = ptr::null_mut();
            }
            (self.api.terminate_destroy)(shared.handle);
        }
    }
    fn name(&self) -> &'static str {
        "libmpv"
    }
}

impl Drop for MpvEngine {
    fn drop(&mut self) {
        self.shutdown();
        // The render thread may still hold the shared state until it exits, so
        // it is deliberately not freed: one small allocation per video.
    }
}

/// mpv's `track-list` property is JSON. This reads the few fields the menus need.
mod track_list {
    pub struct MpvTrack {
        pub id: i64,
        pub kind: String,
        pub title: Option<String>,
        pub lang: Option<String>,
        pub selected: bool,
    }
    pub fn tracks(json: &str) -> Vec<MpvTrack> {
        let Ok(serde_json::Value::Array(items)) = serde_json::from_str(json) else {
            return vec![];
        };
        items
            .iter()
            .filter_map(|item| {
                Some(MpvTrack {
                    id: item.get("id")?.as_i64()?,
                    kind: item.get("type")?.as_str()?.to_owned(),
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    lang: item.get("lang").and_then(|v| v.as_str()).map(str::to_owned),
                    selected: item
                        .get("selected")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                })
            })
            .collect()
    }
}
