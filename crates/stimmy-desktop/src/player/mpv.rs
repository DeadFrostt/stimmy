//! libmpv: MKV and the other formats AVFoundation can't open.
//!
//! The library is opened at runtime, so the app runs without it and simply says
//! so if a video needs it. Frames render through mpv's render API into a
//! CAOpenGLLayer, as IINA does, which keeps hardware decoding and libass.
#![allow(deprecated)]

use super::{Engine, HostView, Snapshot, Track};
use libloading::Library;
use objc2::{
    AnyThread, DefinedClass, class, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, NSObjectProtocol},
};
use objc2_core_foundation::CFTimeInterval;
use objc2_core_video::CVTimeStamp;
use objc2_open_gl::{CGLContextObj, CGLPixelFormatObj};
use objc2_quartz_core::CAOpenGLLayer;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

type Handle = *mut c_void;
type RenderContext = *mut c_void;

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct OpenGlInit {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ctx: *mut c_void,
}
#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
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
const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const PARAM_OPENGL_FBO: c_int = 3;
const PARAM_FLIP_Y: c_int = 4;
const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const EVENT_END_FILE: c_int = 7;
const END_FILE_REASON_ERROR: c_int = 4;

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
        // Searches this binary's rpaths in order: see build.rs.
        "@rpath/libmpv.2.dylib".to_owned(),
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
        "This video needs the libmpv engine, which isn’t installed. Install IINA or `brew install mpv`, then try again."
    })
}

pub fn available() -> bool {
    api().is_ok()
}

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(
        attributes: *const c_int,
        format: *mut CGLPixelFormatObj,
        count: *mut c_int,
    ) -> c_int;
    fn CGLSetCurrentContext(context: CGLContextObj) -> c_int;
    fn CGLLockContext(context: CGLContextObj) -> c_int;
    fn CGLUnlockContext(context: CGLContextObj) -> c_int;
    fn glGetIntegerv(name: u32, out: *mut c_int);
    fn glFlush();
}

const GL_VIEWPORT: u32 = 0x0BA2;
const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

unsafe extern "C" fn get_proc_address(_: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { dlsym(RTLD_DEFAULT, name) }
}

/// Shared with the layer's render thread.
struct Shared {
    handle: Handle,
    render: Mutex<RenderContext>,
    context: AtomicPtr<c_void>,
    needs_draw: AtomicBool,
    first_frame: AtomicBool,
    closed: AtomicBool,
}
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

unsafe extern "C" fn on_update(shared: *mut c_void) {
    // Called from mpv's threads, so nothing here may call back into mpv.
    let shared = unsafe { &*(shared as *const Shared) };
    shared.needs_draw.store(true, Ordering::Release);
}

struct LayerIvars {
    shared: *const Shared,
}

define_class!(
    #[unsafe(super(CAOpenGLLayer))]
    #[name = "StimmyMpvLayer"]
    #[ivars = LayerIvars]
    struct MpvLayer;

    unsafe impl NSObjectProtocol for MpvLayer {}

    impl MpvLayer {
        #[unsafe(method(copyCGLPixelFormatForDisplayMask:))]
        fn copy_pixel_format(&self, _mask: u32) -> CGLPixelFormatObj {
            // A 3.2 core context, which mpv's video shaders and hardware interop expect.
            const ACCELERATED: c_int = 73;
            const DOUBLE_BUFFER: c_int = 5;
            const PROFILE: c_int = 99;
            const CORE_3_2: c_int = 0x3200;
            let attributes = [PROFILE, CORE_3_2, ACCELERATED, DOUBLE_BUFFER, 0];
            let mut format: CGLPixelFormatObj = ptr::null_mut();
            let mut count = 0;
            unsafe { CGLChoosePixelFormat(attributes.as_ptr(), &mut format, &mut count) };
            format
        }

        #[unsafe(method(canDrawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn can_draw(&self, _c: CGLContextObj, _p: CGLPixelFormatObj, _t: CFTimeInterval, _s: *const CVTimeStamp) -> bool {
            let shared = unsafe { &*self.ivars().shared };
            !shared.closed.load(Ordering::Acquire)
                && (shared.needs_draw.load(Ordering::Acquire)
                    || shared.render.lock().map(|r| r.is_null()).unwrap_or(false))
        }

        #[unsafe(method(drawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn draw(&self, context: CGLContextObj, format: CGLPixelFormatObj, time: CFTimeInterval, stamp: *const CVTimeStamp) {
            self.draw_frame(context);
            let _: () = unsafe {
                msg_send![super(self), drawInCGLContext: context, pixelFormat: format, forLayerTime: time, displayTime: stamp]
            };
        }
    }
);

impl MpvLayer {
    fn new(shared: *const Shared) -> Retained<Self> {
        let this: Allocated<Self> = Self::alloc();
        let this = this.set_ivars(LayerIvars { shared });
        unsafe { msg_send![super(this), init] }
    }

    fn draw_frame(&self, context: CGLContextObj) {
        let Ok(api) = api() else {
            return;
        };
        let shared = unsafe { &*self.ivars().shared };
        if shared.closed.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut render) = shared.render.lock() else {
            return;
        };
        unsafe {
            if render.is_null() {
                let mut init = OpenGlInit {
                    get_proc_address,
                    ctx: ptr::null_mut(),
                };
                let mut params = [
                    RenderParam {
                        kind: PARAM_API_TYPE,
                        data: c"opengl".as_ptr() as *mut c_void,
                    },
                    RenderParam {
                        kind: PARAM_OPENGL_INIT_PARAMS,
                        data: &mut init as *mut _ as *mut c_void,
                    },
                    RenderParam {
                        kind: PARAM_INVALID,
                        data: ptr::null_mut(),
                    },
                ];
                if (api.render_create)(&mut *render, shared.handle, params.as_mut_ptr()) < 0 {
                    *render = ptr::null_mut();
                    return;
                }
                shared
                    .context
                    .store(context as *mut c_void, Ordering::Release);
                (api.render_update_callback)(
                    *render,
                    Some(on_update),
                    self.ivars().shared as *mut c_void,
                );
            }
            shared.needs_draw.store(false, Ordering::Release);
            let flags = (api.render_update)(*render);
            let mut viewport = [0; 4];
            glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
            let mut framebuffer = 0;
            glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut framebuffer);
            let mut fbo = OpenGlFbo {
                fbo: framebuffer,
                w: viewport[2],
                h: viewport[3],
                internal_format: 0,
            };
            let mut flip: c_int = 1;
            let mut block: c_int = 0;
            let mut params = [
                RenderParam {
                    kind: PARAM_OPENGL_FBO,
                    data: &mut fbo as *mut _ as *mut c_void,
                },
                RenderParam {
                    kind: PARAM_FLIP_Y,
                    data: &mut flip as *mut _ as *mut c_void,
                },
                RenderParam {
                    kind: PARAM_BLOCK_FOR_TARGET_TIME,
                    data: &mut block as *mut _ as *mut c_void,
                },
                RenderParam {
                    kind: PARAM_INVALID,
                    data: ptr::null_mut(),
                },
            ];
            (api.render_render)(*render, params.as_mut_ptr());
            glFlush();
            if flags & 1 != 0 {
                shared.first_frame.store(true, Ordering::Release);
            }
        }
    }
}

fn cstring(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

pub struct MpvEngine {
    api: &'static Api,
    shared: *const Shared,
    layer: Retained<MpvLayer>,
    view: Retained<AnyObject>,
    pending: Option<CString>,
    failed: Option<String>,
    closed: bool,
}

impl MpvEngine {
    pub fn open(host: HostView, url: &str, start: f64) -> Result<Self, &'static str> {
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
                ("hwdec", "videotoolbox"),
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
            let shared = Box::into_raw(Box::new(Shared {
                handle,
                render: Mutex::new(ptr::null_mut()),
                context: AtomicPtr::new(ptr::null_mut()),
                needs_draw: AtomicBool::new(true),
                first_frame: AtomicBool::new(false),
                closed: AtomicBool::new(false),
            }));
            let layer = MpvLayer::new(shared);
            layer.setAsynchronous(true);
            let _: () = msg_send![&layer, setContentsScale: host.backing_scale()];
            let _: () = msg_send![&layer, setAutoresizingMask: 2u32 | 16u32];
            let _: () = msg_send![&layer, setNeedsDisplayOnBoundsChange: true];
            let view: Retained<AnyObject> = msg_send![class!(NSView), new];
            let _: () = msg_send![&view, setWantsLayer: true];
            let backing: *mut AnyObject = msg_send![&view, layer];
            host.insert_below(Retained::as_ptr(&view) as *mut AnyObject);
            let bounds: objc2_core_foundation::CGRect = msg_send![&view, bounds];
            let _: () = msg_send![&layer, setFrame: bounds];
            let _: () = msg_send![backing, addSublayer: &*layer];
            // Loading waits for the render context: `vo=libmpv` can't start without
            // one, and the context only exists after the layer's first GL draw.
            Ok(Self {
                api,
                shared,
                layer,
                view,
                pending: Some(cstring(url)),
                failed: None,
                closed: false,
            })
        }
    }

    fn load_when_ready(&mut self) {
        let ready = self
            .shared()
            .render
            .lock()
            .is_ok_and(|render| !render.is_null());
        if !ready {
            return;
        }
        let Some(url) = self.pending.take() else {
            return;
        };
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
        self.load_when_ready();
        self.drain_events();
        let (audio, subtitles) = self.tracks();
        let position = self.number("time-pos");
        let shared = self.shared();
        Snapshot {
            position,
            duration: self.number("duration"),
            buffered: position + self.number("demuxer-cache-duration"),
            paused: self.flag("pause"),
            buffering: self.flag("paused-for-cache")
                || self.flag("seeking")
                || !shared.first_frame.load(Ordering::Acquire),
            ended: self.flag("eof-reached"),
            first_frame: shared.first_frame.load(Ordering::Acquire),
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
    fn shutdown(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        let shared = self.shared();
        // Stop the render thread first, then release in mpv's required order:
        // the render context needs its GL context current, and goes before mpv.
        shared.closed.store(true, Ordering::Release);
        self.layer.setAsynchronous(false);
        unsafe {
            let _: () = msg_send![&self.layer, removeFromSuperlayer];
            let _: () = msg_send![&self.view, removeFromSuperview];
            if let Ok(mut render) = shared.render.lock()
                && !render.is_null()
            {
                let context = shared.context.load(Ordering::Acquire) as CGLContextObj;
                CGLLockContext(context);
                CGLSetCurrentContext(context);
                (self.api.render_update_callback)(*render, None, ptr::null_mut());
                (self.api.render_free)(*render);
                *render = ptr::null_mut();
                CGLSetCurrentContext(ptr::null_mut());
                CGLUnlockContext(context);
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
        // The layer may be referenced until Core Animation lets it go, so its
        // shared state is deliberately not freed: one small allocation per video.
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
