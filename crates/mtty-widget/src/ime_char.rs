//! Windows IME quick-commit hook.
//!
//! Some IMEs commit a character without ever starting a composition -
//! Microsoft Pinyin's full-width punctuation is the common case. Windows
//! posts `WM_IME_CHAR` straight to the window for those. winit 0.30 has no
//! handler for the message and its `WM_CHAR` branch drops characters with
//! no matching key press, so the commit never becomes an event and the
//! character is lost: full-width quotes cannot be typed into a pane.
//!
//! Subclassing the window observes `WM_IME_CHAR` first and forwards each
//! committed character as `HostEvent::ImeChar`, which the loop delivers
//! through the same path as a composition commit. The message is swallowed
//! so `DefWindowProc` cannot re-emit it as a duplicate `WM_CHAR`.

use std::cell::Cell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_IME_CHAR, WM_NCDESTROY};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::HostEvent;

const SUBCLASS_ID: usize = 1;

struct HookCtx {
    proxy: EventLoopProxy<HostEvent>,
    /// High surrogate awaiting the second message of a UTF-16 pair.
    pending_high_surrogate: Cell<Option<u16>>,
}

/// Observe `WM_IME_CHAR` on `window` and forward every committed character.
pub fn install(window: &Window, proxy: EventLoopProxy<HostEvent>) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as *mut core::ffi::c_void;
    let ctx = Box::into_raw(Box::new(HookCtx {
        proxy,
        pending_high_surrogate: Cell::new(None),
    }));
    // SAFETY: the subclass lives as long as the window; the hook reclaims
    // `ctx` when the window is destroyed (WM_NCDESTROY below).
    unsafe {
        SetWindowSubclass(hwnd, Some(ime_char_proc), SUBCLASS_ID, ctx as usize);
    }
}

unsafe extern "system" fn ime_char_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    ref_data: usize,
) -> LRESULT {
    if ref_data == 0 {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    }
    let ctx = unsafe { &*(ref_data as *const HookCtx) };
    if msg == WM_NCDESTROY {
        // SAFETY: the window is going away; reclaim the context so it does
        // not leak.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(ime_char_proc), SUBCLASS_ID);
            drop(Box::from_raw(ref_data as *mut HookCtx));
        }
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    }
    if msg == WM_IME_CHAR {
        let unit = wparam as u16;
        let committed = if (0xd800..0xdc00).contains(&unit) {
            // High surrogate: wait for the pairing message.
            ctx.pending_high_surrogate.set(Some(unit));
            None
        } else {
            let text = match ctx.pending_high_surrogate.take() {
                Some(high) => char::decode_utf16([high, unit]).next().and_then(|r| r.ok()),
                None => char::from_u32(unit as u32),
            };
            if text.is_none() {
                // A lone tail surrogate: nothing to deliver.
                ctx.pending_high_surrogate.set(None);
            }
            text
        };
        if let Some(ch) = committed {
            let _ = ctx.proxy.send_event(HostEvent::ImeChar(ch));
            // Swallow the message: forwarding it would land in
            // DefWindowProc, which posts the same character again as a
            // WM_CHAR the loop would count twice.
            return 0;
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
