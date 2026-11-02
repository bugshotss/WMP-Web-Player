//! Core Audio declarations the volume and the audio capture share.

use std::ffi::c_void;

#[repr(C)]
pub struct PropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    pub fn AudioObjectGetPropertyData(
        id: u32,
        address: *const PropertyAddress,
        qsize: u32,
        qdata: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    pub fn AudioObjectSetPropertyData(
        id: u32,
        address: *const PropertyAddress,
        qsize: u32,
        qdata: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

pub const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub const SYSTEM_OBJECT: u32 = 1;
pub const GLOBAL: u32 = fourcc(b"glob");
pub const OUTPUT: u32 = fourcc(b"outp");

pub fn default_output() -> Option<u32> {
    let addr = PropertyAddress { selector: fourcc(b"dOut"), scope: GLOBAL, element: 0 };
    let mut id = 0u32;
    let mut size = 4u32;
    let status = unsafe { AudioObjectGetPropertyData(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size, (&raw mut id).cast()) };
    (status == 0 && id != 0).then_some(id)
}
