//! Text slots of the RivaTuner Statistics Server on-screen display, through its shared memory.
//!
//! Layout (RTSSSharedMemory.h, v2.x): a header with the offset, count and size of the OSD entries,
//! then the entries. Each entry is `szOSD[256]`, `szOSDOwner[256]`, `szOSDEx[4096]`, ...
//! Slot 0 belongs to the primary client (MSI Afterburner), others start at 1.

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS,
};

const SIGNATURE: u32 = 0x5254_5353; // 'RTSS'
const OFF_SIGNATURE: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_OSD_ENTRY_SIZE: usize = 20;
const OFF_OSD_ARR_OFFSET: usize = 24;
const OFF_OSD_ARR_SIZE: usize = 28;
const OFF_OSD_FRAME: usize = 32;
const OFF_BUSY: usize = 36;
const OFF_APP_ENTRY_SIZE: usize = 8;
const OFF_APP_ARR_OFFSET: usize = 12;
const OFF_APP_ARR_SIZE: usize = 16;
/// v2.16+: index and process ID of the last foreground application.
const OFF_LAST_FOREGROUND_PID: usize = 68;

/// Fields of an application entry: process ID, end of the last framerate period, OSD zoom.
const APP_PID: usize = 0;
const APP_TIME1: usize = 272;
const APP_OSD_PIXEL: usize = 324;

const ENTRY_OSD: usize = 0;
const ENTRY_OWNER: usize = 256;
const ENTRY_OSD_EX: usize = 512;
const LEN_OSD: usize = 256;
const LEN_OWNER: usize = 256;
const LEN_OSD_EX: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Update {
    /// The text is in the slot.
    Written,
    /// The renderer holds the OSD lock right now, try again shortly.
    Busy,
    /// RTSS isn't running, or every slot is taken.
    Unavailable,
}

struct View {
    handle: HANDLE,
    addr: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl View {
    fn open() -> Option<View> {
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("RTSSSharedMemoryV2")).ok()?;
            let addr = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if addr.Value.is_null() {
                let _ = CloseHandle(handle);
                return None;
            }
            let view = View { handle, addr };
            (view.u32_at(OFF_SIGNATURE) == SIGNATURE && view.u32_at(OFF_VERSION) >= 0x0002_0000).then_some(view)
        }
    }

    fn base(&self) -> *mut u8 {
        self.addr.Value as *mut u8
    }

    unsafe fn u32_at(&self, off: usize) -> u32 {
        unsafe { (self.base().add(off) as *const u32).read_volatile() }
    }

    unsafe fn bump_frame(&self) {
        unsafe {
            let p = self.base().add(OFF_OSD_FRAME) as *mut u32;
            p.write_volatile(p.read_volatile().wrapping_add(1));
        }
    }

    /// Entries from the second one on, as raw pointers with their size.
    unsafe fn entries(&self) -> impl Iterator<Item = *mut u8> + '_ {
        unsafe {
            let size = self.u32_at(OFF_OSD_ENTRY_SIZE) as usize;
            let offset = self.u32_at(OFF_OSD_ARR_OFFSET) as usize;
            let count = self.u32_at(OFF_OSD_ARR_SIZE) as usize;
            (1..count).map(move |i| self.base().add(offset + i * size))
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.addr);
            let _ = CloseHandle(self.handle);
        }
    }
}

unsafe fn c_str_eq(p: *const u8, max: usize, s: &str) -> bool {
    unsafe {
        let bytes = std::slice::from_raw_parts(p, max);
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(max);
        &bytes[..len] == s.as_bytes()
    }
}

unsafe fn write_c_str(p: *mut u8, max: usize, s: &str) {
    unsafe {
        let n = s.len().min(max - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr(), p, n);
        p.add(n).write(0);
    }
}

/// Puts `text` into the slot owned by `owner`, taking a free slot if it has none yet.
pub fn update(owner: &str, text: &str) -> Update {
    let Some(view) = View::open() else { return Update::Unavailable };
    unsafe {
        let version = view.u32_at(OFF_VERSION);
        let mut slot = view.entries().find(|&e| c_str_eq(e.add(ENTRY_OWNER), LEN_OWNER, owner));
        if slot.is_none() {
            slot = view.entries().find(|&e| *e.add(ENTRY_OWNER) == 0);
            if let Some(e) = slot {
                write_c_str(e.add(ENTRY_OWNER), LEN_OWNER, owner);
            }
        }
        let Some(entry) = slot else { return Update::Unavailable };

        if version >= 0x0002_0007 {
            // v2.7+: the extended slot holds 4096 characters. v2.14+: take the OSD lock first, so
            // the renderer never reads a half-written text.
            if version >= 0x0002_000e {
                let busy = AtomicU32::from_ptr(view.base().add(OFF_BUSY) as *mut u32);
                if busy.fetch_or(1, Ordering::SeqCst) & 1 != 0 {
                    return Update::Busy;
                }
                write_c_str(entry.add(ENTRY_OSD_EX), LEN_OSD_EX, text);
                busy.store(0, Ordering::SeqCst);
            } else {
                write_c_str(entry.add(ENTRY_OSD_EX), LEN_OSD_EX, text);
            }
        } else {
            write_c_str(entry.add(ENTRY_OSD), LEN_OSD, text);
        }
        view.bump_frame();
    }
    Update::Written
}

/// True when the slot owned by `owner` exists and holds exactly `text`.
pub fn holds(owner: &str, text: &str) -> bool {
    let Some(view) = View::open() else { return false };
    unsafe {
        let ex = view.u32_at(OFF_VERSION) >= 0x0002_0007;
        view.entries().any(|e| {
            c_str_eq(e.add(ENTRY_OWNER), LEN_OWNER, owner)
                && if ex { c_str_eq(e.add(ENTRY_OSD_EX), LEN_OSD_EX, text) } else { c_str_eq(e.add(ENTRY_OSD), LEN_OSD, text) }
        })
    }
}

/// The "On-Screen Display zoom" of the game the crosshair is for: the last foreground 3D
/// application, or, when RTSS's record of that is stale, the one that rendered most recently.
/// `None` when no game is hooked right now.
pub fn game_zoom() -> Option<u32> {
    let view = View::open()?;
    unsafe {
        let size = view.u32_at(OFF_APP_ENTRY_SIZE) as usize;
        let offset = view.u32_at(OFF_APP_ARR_OFFSET) as usize;
        let count = view.u32_at(OFF_APP_ARR_SIZE) as usize;
        if size < APP_OSD_PIXEL + 4 {
            return None;
        }
        let foreground = if view.u32_at(OFF_VERSION) >= 0x0002_0010 { view.u32_at(OFF_LAST_FOREGROUND_PID) } else { 0 };
        let apps: Vec<(u32, u32, u32)> = (0..count)
            .map(|i| {
                let e = offset + i * size;
                (view.u32_at(e + APP_PID), view.u32_at(e + APP_TIME1), view.u32_at(e + APP_OSD_PIXEL))
            })
            .filter(|a| a.0 != 0)
            .collect();
        let app = apps.iter().find(|a| a.0 == foreground).or_else(|| apps.iter().max_by_key(|a| a.1))?;
        Some(app.2.clamp(1, 16))
    }
}

/// Frees every slot owned by `owner`.
pub fn release(owner: &str) {
    let Some(view) = View::open() else { return };
    unsafe {
        let size = view.u32_at(OFF_OSD_ENTRY_SIZE) as usize;
        let mut freed = false;
        for e in view.entries() {
            if c_str_eq(e.add(ENTRY_OWNER), LEN_OWNER, owner) {
                std::ptr::write_bytes(e, 0, size);
                freed = true;
            }
        }
        if freed {
            view.bump_frame();
        }
    }
}
