//! The Mac's output volume and mute. macOS has no per-app mixer, so a skin's volume slider works
//! the system volume, like the keyboard's volume keys.

use super::core_audio::{AudioObjectGetPropertyData, AudioObjectSetPropertyData, OUTPUT, PropertyAddress, default_output, fourcc};
use std::ffi::c_void;

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioHardwareServiceGetPropertyData(
        id: u32,
        address: *const PropertyAddress,
        qsize: u32,
        qdata: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioHardwareServiceSetPropertyData(
        id: u32,
        address: *const PropertyAddress,
        qsize: u32,
        qdata: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

const VIRTUAL_MAIN_VOLUME: PropertyAddress = PropertyAddress { selector: fourcc(b"vmvc"), scope: OUTPUT, element: 0 };
const MUTE: PropertyAddress = PropertyAddress { selector: fourcc(b"mute"), scope: OUTPUT, element: 0 };

/// 0..1, or None when the output has no volume control.
pub fn get() -> Option<f64> {
    let dev = default_output()?;
    let mut v = 0f32;
    let mut size = 4u32;
    let status = unsafe { AudioHardwareServiceGetPropertyData(dev, &VIRTUAL_MAIN_VOLUME, 0, std::ptr::null(), &mut size, (&raw mut v).cast()) };
    (status == 0).then_some(f64::from(v))
}

pub fn set(v: f64) {
    let Some(dev) = default_output() else { return };
    let v = v.clamp(0.0, 1.0) as f32;
    unsafe { AudioHardwareServiceSetPropertyData(dev, &VIRTUAL_MAIN_VOLUME, 0, std::ptr::null(), 4, (&raw const v).cast()) };
}

pub fn muted() -> Option<bool> {
    let dev = default_output()?;
    let mut m = 0u32;
    let mut size = 4u32;
    let status = unsafe { AudioObjectGetPropertyData(dev, &MUTE, 0, std::ptr::null(), &mut size, (&raw mut m).cast()) };
    (status == 0).then_some(m != 0)
}

pub fn set_muted(on: bool) {
    let Some(dev) = default_output() else { return };
    let m = u32::from(on);
    unsafe { AudioObjectSetPropertyData(dev, &MUTE, 0, std::ptr::null(), 4, (&raw const m).cast()) };
}
