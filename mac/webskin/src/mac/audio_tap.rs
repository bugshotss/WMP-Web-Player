//! What the Mac is playing, for the visualizations: a Core Audio process tap on all system
//! output (macOS 14.2 and later), read through a private aggregate device. macOS asks the user
//! once for "System Audio Recording" permission; without it the tap delivers silence.

use super::core_audio::{AudioObjectGetPropertyData, GLOBAL, PropertyAddress, default_output, fourcc};
use crate::vis::AudioRing;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{Message, msg_send};
use objc2_foundation::{NSArray, NSCopying, NSMutableDictionary, NSNumber, NSObject, NSString};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    bytes: u32,
    data: *const f32,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Default)]
struct StreamDescription {
    sample_rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

type IoProc =
    unsafe extern "C" fn(u32, *const c_void, *const AudioBufferList, *const c_void, *mut AudioBufferList, *const c_void, *mut c_void) -> i32;
type CreateProcessTap = unsafe extern "C" fn(*const AnyObject, *mut u32) -> i32;
type DestroyProcessTap = unsafe extern "C" fn(u32) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioHardwareCreateAggregateDevice(description: *const AnyObject, device: *mut u32) -> i32;
    fn AudioHardwareDestroyAggregateDevice(device: u32) -> i32;
    fn AudioDeviceCreateIOProcID(device: u32, proc_: IoProc, client: *mut c_void, id: *mut *mut c_void) -> i32;
    fn AudioDeviceDestroyIOProcID(device: u32, id: *mut c_void) -> i32;
    fn AudioDeviceStart(device: u32, id: *mut c_void) -> i32;
    fn AudioDeviceStop(device: u32, id: *mut c_void) -> i32;
}

/// A running capture. Dropping it stops the capture and removes the tap.
pub struct AudioTap {
    tap: u32,
    device: u32,
    proc_id: *mut c_void,
    ring: *const Mutex<AudioRing>,
    destroy_tap: DestroyProcessTap,
}

impl AudioTap {
    /// Starts capturing into `ring`. The error says why it couldn't (shown in the log).
    pub fn start(ring: Arc<Mutex<AudioRing>>) -> Result<AudioTap, String> {
        // Looked up at run time: on macOS before 14.2 neither exists, and the app still runs.
        let create = symbol::<CreateProcessTap>(c"AudioHardwareCreateProcessTap").ok_or("this macOS has no audio taps (needs 14.2)")?;
        let destroy_tap = symbol::<DestroyProcessTap>(c"AudioHardwareDestroyProcessTap").ok_or("this macOS has no audio taps (needs 14.2)")?;
        let class = AnyClass::get(c"CATapDescription").ok_or("this macOS has no CATapDescription")?;
        unsafe {
            let none = NSArray::<NSNumber>::new();
            let alloc: Allocated<NSObject> = msg_send![class, alloc];
            let description: Option<Retained<NSObject>> = msg_send![alloc, initStereoGlobalTapButExcludeProcesses: &*none];
            let description = description.ok_or("CATapDescription init failed")?;
            let _: () = msg_send![&*description, setPrivate: true];
            // CATapUnmuted: the music keeps playing out loud.
            let _: () = msg_send![&*description, setMuteBehavior: 0isize];
            let mut tap = 0u32;
            check(create(Retained::as_ptr(&description).cast(), &mut tap), "AudioHardwareCreateProcessTap")?;
            let fail = |e: String| {
                destroy_tap(tap);
                e
            };
            let tap_uid = string_property(tap, fourcc(b"tuid")).ok_or_else(|| fail("the tap has no UID".into()))?;
            let output = default_output().and_then(|d| string_property(d, fourcc(b"uid "))).ok_or_else(|| fail("no default output device".into()))?;
            let mut format = StreamDescription::default();
            let mut size = size_of::<StreamDescription>() as u32;
            let addr = PropertyAddress { selector: fourcc(b"tfmt"), scope: GLOBAL, element: 0 };
            check(AudioObjectGetPropertyData(tap, &addr, 0, std::ptr::null(), &mut size, (&raw mut format).cast()), "tap format").map_err(fail)?;

            let description = aggregate_description(&tap_uid, &output);
            let mut device = 0u32;
            check(AudioHardwareCreateAggregateDevice(Retained::as_ptr(&description).cast(), &mut device), "AudioHardwareCreateAggregateDevice")
                .map_err(fail)?;
            if let Ok(mut r) = ring.lock() {
                r.sample_rate = format.sample_rate.round() as u32;
            }
            let ring = Arc::into_raw(ring);
            let mut proc_id = std::ptr::null_mut();
            let started = check(AudioDeviceCreateIOProcID(device, io_proc, ring.cast_mut().cast(), &mut proc_id), "AudioDeviceCreateIOProcID")
                .and_then(|()| check(AudioDeviceStart(device, proc_id), "AudioDeviceStart"));
            let me = AudioTap { tap, device, proc_id, ring, destroy_tap };
            started.map(|()| me)
        }
    }
}

impl Drop for AudioTap {
    fn drop(&mut self) {
        unsafe {
            if !self.proc_id.is_null() {
                AudioDeviceStop(self.device, self.proc_id);
                AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            AudioHardwareDestroyAggregateDevice(self.device);
            (self.destroy_tap)(self.tap);
            drop(Arc::from_raw(self.ring));
        }
    }
}

/// Runs on Core Audio's real-time thread for every block of tapped audio (32-bit float).
unsafe extern "C" fn io_proc(
    _device: u32,
    _now: *const c_void,
    input: *const AudioBufferList,
    _input_time: *const c_void,
    _output: *mut AudioBufferList,
    _output_time: *const c_void,
    client: *mut c_void,
) -> i32 {
    let (Some(list), Some(ring)) = (unsafe { input.as_ref() }, unsafe { client.cast::<Mutex<AudioRing>>().as_ref() }) else { return 0 };
    let buffers = unsafe { std::slice::from_raw_parts(list.buffers.as_ptr(), list.count as usize) };
    let samples = |b: &AudioBuffer| {
        if b.data.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(b.data, b.bytes as usize / 4) } }
    };
    let Ok(mut ring) = ring.try_lock() else { return 0 };
    match buffers {
        // Non-interleaved stereo: one buffer per channel.
        [left, right, ..] if left.channels == 1 => {
            let interleaved: Vec<f32> = samples(left).iter().zip(samples(right)).flat_map(|(&l, &r)| [l, r]).collect();
            ring.push_interleaved(&interleaved, 2);
        }
        [only, ..] => ring.push_interleaved(samples(only), only.channels as usize),
        [] => {}
    }
    0
}

/// A dictionary of Foundation objects by name.
fn dictionary(entries: Vec<(&str, Retained<NSObject>)>) -> Retained<NSObject> {
    let dict = NSMutableDictionary::<NSString, NSObject>::new();
    for (key, value) in entries {
        let _: () = unsafe { msg_send![&*dict, setObject: &*value, forKey: &*NSString::from_str(key)] };
    }
    object(dict)
}

fn object<T: Message>(o: Retained<T>) -> Retained<NSObject> {
    // Every Foundation object is an NSObject.
    unsafe { Retained::cast_unchecked(o) }
}

/// A private aggregate device made of the default output and the tap, started with the tap.
fn aggregate_description(tap_uid: &NSString, output_uid: &NSString) -> Retained<NSObject> {
    let uid = format!("io.github.bugshotss.webskinplayer.tap.{}", std::process::id());
    let tap = dictionary(vec![("uid", object(tap_uid.copy())), ("drift", object(NSNumber::new_i32(1)))]);
    let output = dictionary(vec![("uid", object(output_uid.copy()))]);
    dictionary(vec![
        ("name", object(NSString::from_str("WebSkin Player visualizations"))),
        ("uid", object(NSString::from_str(&uid))),
        ("private", object(NSNumber::new_i32(1))),
        ("stacked", object(NSNumber::new_i32(0))),
        ("tapautostart", object(NSNumber::new_i32(1))),
        ("master", object(output_uid.copy())),
        ("subdevices", object(NSArray::from_retained_slice(&[output]))),
        ("taps", object(NSArray::from_retained_slice(&[tap]))),
    ])
}

/// A CFString property (a UID) of an audio object.
fn string_property(object: u32, selector: u32) -> Option<Retained<NSString>> {
    let addr = PropertyAddress { selector, scope: GLOBAL, element: 0 };
    let mut value: *mut NSString = std::ptr::null_mut();
    let mut size = size_of::<*mut NSString>() as u32;
    let status = unsafe { AudioObjectGetPropertyData(object, &addr, 0, std::ptr::null(), &mut size, (&raw mut value).cast()) };
    // The getter hands over a +1 reference.
    (status == 0).then(|| unsafe { Retained::from_raw(value) }).flatten()
}

fn symbol<T: Copy>(name: &std::ffi::CStr) -> Option<T> {
    let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what} failed ({status})")) }
}
