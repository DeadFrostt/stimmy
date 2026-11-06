//! AVFoundation: MP4, MOV, and HLS, with no extra dependency.

use super::{Engine, HostView, Snapshot, Track};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_core_foundation::CGRect;
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSString, NSURL};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaCharacteristicAudible: *const AnyObject;
    static AVMediaCharacteristicLegible: *const AnyObject;
}

fn seconds(time: CMTime) -> f64 {
    let value = unsafe { time.seconds() };
    if value.is_finite() { value } else { 0. }
}

pub struct AvEngine {
    player: Retained<AnyObject>,
    layer: Retained<AnyObject>,
    view: Retained<AnyObject>,
    paused: bool,
    closed: bool,
}

impl AvEngine {
    pub fn open(host: HostView, url: &str, start: f64) -> Result<Self, String> {
        let url = NSString::from_str(url);
        let url = NSURL::URLWithString(&url).ok_or("This address can’t be played.")?;
        unsafe {
            let player: Retained<AnyObject> = msg_send![class!(AVPlayer), playerWithURL: &*url];
            let view: Retained<AnyObject> = msg_send![class!(NSView), new];
            let _: () = msg_send![&view, setWantsLayer: true];
            let backing: *mut AnyObject = msg_send![&view, layer];
            let layer: Retained<AnyObject> =
                msg_send![class!(AVPlayerLayer), playerLayerWithPlayer: &*player];
            let bounds: CGRect = msg_send![backing, bounds];
            let _: () = msg_send![&layer, setFrame: bounds];
            let _: () = msg_send![&layer, setAutoresizingMask: 2u32 | 16u32];
            let _: () = msg_send![backing, addSublayer: &*layer];
            host.insert_below(Retained::as_ptr(&view) as *mut AnyObject);
            let frame: CGRect = msg_send![&view, bounds];
            let _: () = msg_send![&layer, setFrame: frame];
            let mut engine = Self {
                player,
                layer,
                view,
                paused: false,
                closed: false,
            };
            if start > 0. {
                engine.seek(start);
            }
            let _: () = msg_send![&engine.player, play];
            Ok(engine)
        }
    }

    fn item(&self) -> Option<Retained<AnyObject>> {
        unsafe { msg_send![&self.player, currentItem] }
    }

    fn group(
        &self,
        item: &AnyObject,
        characteristic: *const AnyObject,
    ) -> Option<Retained<AnyObject>> {
        unsafe {
            let asset: Retained<AnyObject> = msg_send![item, asset];
            msg_send![&asset, mediaSelectionGroupForMediaCharacteristic: characteristic]
        }
    }

    fn tracks(&self, item: &AnyObject, characteristic: *const AnyObject) -> Vec<Track> {
        let Some(group) = self.group(item, characteristic) else {
            return vec![];
        };
        unsafe {
            let options: Retained<NSArray<AnyObject>> = msg_send![&group, options];
            let selection: Retained<AnyObject> = msg_send![item, currentMediaSelection];
            let selected: Option<Retained<AnyObject>> =
                msg_send![&selection, selectedMediaOptionInMediaSelectionGroup: &*group];
            options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let name: Retained<NSString> = msg_send![&option, displayName];
                    Track {
                        id: index.to_string(),
                        name: name.to_string(),
                        selected: selected
                            .as_ref()
                            .is_some_and(|s| Retained::as_ptr(s) == Retained::as_ptr(&option)),
                    }
                })
                .collect()
        }
    }

    fn select(&self, characteristic: *const AnyObject, id: Option<&str>) {
        let Some(item) = self.item() else {
            return;
        };
        let Some(group) = self.group(&item, characteristic) else {
            return;
        };
        unsafe {
            let options: Retained<NSArray<AnyObject>> = msg_send![&group, options];
            let option = id
                .and_then(|id| id.parse::<usize>().ok())
                .filter(|index| *index < options.count())
                .map(|index| options.objectAtIndex(index));
            let raw: *const AnyObject = option.as_ref().map_or(std::ptr::null(), Retained::as_ptr);
            let _: () = msg_send![&item, selectMediaOption: raw, inMediaSelectionGroup: &*group];
        }
    }
}

impl Engine for AvEngine {
    fn snapshot(&mut self) -> Snapshot {
        let mut snapshot = Snapshot {
            paused: self.paused,
            ..Default::default()
        };
        let Some(item) = self.item() else {
            return snapshot;
        };
        unsafe {
            let status: isize = msg_send![&item, status];
            if status == 2 {
                // The underlying error can quote a configured URL, so it isn't shown.
                snapshot.failed = Some("This format couldn’t be opened.".into());
                return snapshot;
            }
            let position: CMTime = msg_send![&self.player, currentTime];
            let duration: CMTime = msg_send![&item, duration];
            snapshot.position = seconds(position);
            snapshot.duration = seconds(duration);
            let control: isize = msg_send![&self.player, timeControlStatus];
            snapshot.buffering = control == 1 || status == 0;
            snapshot.first_frame = msg_send![&self.layer, isReadyForDisplay];
            snapshot.ended = snapshot.duration > 0. && snapshot.position >= snapshot.duration - 0.3;
            if status == 1 {
                snapshot.audio = self.tracks(&item, AVMediaCharacteristicAudible);
                snapshot.subtitles = self.tracks(&item, AVMediaCharacteristicLegible);
            }
        }
        snapshot
    }
    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        unsafe {
            if paused {
                let _: () = msg_send![&self.player, pause];
            } else {
                let _: () = msg_send![&self.player, play];
            }
        }
    }
    fn seek(&mut self, target: f64) {
        unsafe {
            let time = CMTime::with_seconds(target.max(0.), 600);
            let exact = CMTime::with_seconds(0.25, 600);
            let _: () = msg_send![&self.player, seekToTime: time, toleranceBefore: exact, toleranceAfter: exact];
        }
    }
    fn set_volume(&mut self, volume: f32) {
        unsafe {
            let _: () = msg_send![&self.player, setVolume: volume.clamp(0., 1.)];
        }
    }
    fn set_speed(&mut self, speed: f32) {
        unsafe {
            let _: () = msg_send![&self.player, setDefaultRate: speed];
            if !self.paused {
                let _: () = msg_send![&self.player, setRate: speed];
            }
        }
    }
    fn select_audio(&mut self, id: &str) {
        self.select(unsafe { AVMediaCharacteristicAudible }, Some(id));
    }
    fn select_subtitle(&mut self, id: Option<&str>) {
        self.select(unsafe { AVMediaCharacteristicLegible }, id);
    }
    fn shutdown(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        unsafe {
            let _: () = msg_send![&self.player, pause];
            let none: *const AnyObject = std::ptr::null();
            let _: () = msg_send![&self.player, replaceCurrentItemWithPlayerItem: none];
            let _: () = msg_send![&self.layer, setPlayer: none];
            let _: () = msg_send![&self.layer, removeFromSuperlayer];
            let _: () = msg_send![&self.view, removeFromSuperview];
        }
    }
    fn name(&self) -> &'static str {
        "AVFoundation"
    }
}

impl Drop for AvEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
