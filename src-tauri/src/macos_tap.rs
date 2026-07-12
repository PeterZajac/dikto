//! Raw CGEventTap keyboard listener for macOS.
//!
//! Replaces rdev::listen, whose macOS callback calls
//! `Keyboard::create_string_for_key` -> `TISCopyCurrentKeyboardInputSource` /
//! `UCKeyTranslate` (HIToolbox) on every KeyPress, on the listener thread. On
//! macOS 15 HIToolbox asserts main-thread via `dispatch_assert_queue`, so that
//! call kills the app (EXC_BREAKPOINT) on the first keystroke after
//! Accessibility is granted. This module never touches HIToolbox — keycodes
//! are named from a static table, not by asking the system to translate them.

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// A key transition captured off the tap, named the same way rdev's `Key`
/// Debug format does (e.g. "AltGr", "KeyA") so it matches what's already
/// stored in settings.json.
pub enum TapEvent {
    Down(String),
    Up(String),
}

// core-graphics's binding to CGEventTapEnable is private to that crate; this
// is the same system function, declared locally so a disabled tap (timeout or
// suspicious-input heuristic) can re-enable itself.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

/// Blocks forever pumping a CGEventTap-driven run loop, calling `callback`
/// for every non-autorepeat key down/up — including modifier-only keys,
/// derived from FlagsChanged. Returns `Err` only if the tap couldn't be
/// created, e.g. missing Accessibility permission; the caller never sees
/// this thread again once it returns `Ok` (it doesn't, in practice — the run
/// loop runs until the process exits).
pub fn listen<F>(callback: F) -> Result<(), String>
where
    F: FnMut(TapEvent) + Send + 'static,
{
    let callback = Mutex::new(callback);

    // Modifier keys report only FlagsChanged, with no down/up flag of their
    // own: a keycode already in this set means the event is that key's
    // release, otherwise it's a press.
    let held_modifiers: Mutex<HashSet<i64>> = Mutex::new(HashSet::new());

    // Filled in right after the tap is created, so the callback can
    // re-enable it without capturing the `CGEventTap` value itself (which
    // would be a reference cycle — the tap owns this callback).
    let mach_port_addr = std::sync::Arc::new(AtomicUsize::new(0));
    let mach_port_addr_cb = mach_port_addr.clone();

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged],
        move |_proxy, etype, event| {
            match etype {
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    let addr = mach_port_addr_cb.load(Ordering::SeqCst);
                    if addr != 0 {
                        unsafe { CGEventTapEnable(addr as CFMachPortRef, true) };
                    }
                }
                CGEventType::KeyDown => {
                    if event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) == 0 {
                        let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        let mut cb = callback.lock().unwrap();
                        (*cb)(TapEvent::Down(key_name(code)));
                    }
                }
                CGEventType::KeyUp => {
                    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let mut cb = callback.lock().unwrap();
                    (*cb)(TapEvent::Up(key_name(code)));
                }
                CGEventType::FlagsChanged => {
                    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let is_release = {
                        let mut held = held_modifiers.lock().unwrap();
                        let was_held = held.remove(&code);
                        if !was_held {
                            held.insert(code);
                        }
                        was_held
                    };
                    let name = key_name(code);
                    let mut cb = callback.lock().unwrap();
                    (*cb)(if is_release { TapEvent::Up(name) } else { TapEvent::Down(name) });
                }
                _ => {}
            }
            CallbackResult::Keep
        },
    )
    .map_err(|_| "CGEventTapCreate failed (missing Accessibility permission?)".to_string())?;

    mach_port_addr.store(tap.mach_port().as_concrete_TypeRef() as usize, Ordering::SeqCst);

    let loop_source = tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|_| "CFMachPortCreateRunLoopSource failed".to_string())?;
    CFRunLoop::get_current().add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    CFRunLoop::run_current();
    Ok(())
}

/// Maps a macOS virtual keycode to the same name rdev's `Key` Debug format
/// would produce, so it matches what settings.json already stores. Table
/// copied from rdev 0.5.3's `src/macos/keycodes.rs`. Unknown codes still
/// round-trip through settings/capture as "Unknown(<code>)".
fn key_name(code: i64) -> String {
    let name = match code {
        0 => "KeyA",
        1 => "KeyS",
        2 => "KeyD",
        3 => "KeyF",
        4 => "KeyH",
        5 => "KeyG",
        6 => "KeyZ",
        7 => "KeyX",
        8 => "KeyC",
        9 => "KeyV",
        11 => "KeyB",
        12 => "KeyQ",
        13 => "KeyW",
        14 => "KeyE",
        15 => "KeyR",
        16 => "KeyY",
        17 => "KeyT",
        18 => "Num1",
        19 => "Num2",
        20 => "Num3",
        21 => "Num4",
        22 => "Num6",
        23 => "Num5",
        24 => "Equal",
        25 => "Num9",
        26 => "Num7",
        27 => "Minus",
        28 => "Num8",
        29 => "Num0",
        30 => "RightBracket",
        31 => "KeyO",
        32 => "KeyU",
        33 => "LeftBracket",
        34 => "KeyI",
        35 => "KeyP",
        36 => "Return",
        37 => "KeyL",
        38 => "KeyJ",
        39 => "Quote",
        40 => "KeyK",
        41 => "SemiColon",
        42 => "BackSlash",
        43 => "Comma",
        44 => "Slash",
        45 => "KeyN",
        46 => "KeyM",
        47 => "Dot",
        48 => "Tab",
        49 => "Space",
        50 => "BackQuote",
        51 => "Backspace",
        53 => "Escape",
        54 => "MetaRight",
        55 => "MetaLeft",
        56 => "ShiftLeft",
        57 => "CapsLock",
        58 => "Alt",
        59 => "ControlLeft",
        60 => "ShiftRight",
        61 => "AltGr",
        62 => "ControlRight",
        63 => "Function",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        103 => "F11",
        109 => "F10",
        111 => "F12",
        118 => "F4",
        120 => "F2",
        122 => "F1",
        123 => "LeftArrow",
        124 => "RightArrow",
        125 => "DownArrow",
        126 => "UpArrow",
        other => return format!("Unknown({other})"),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_mapping_matches_rdev_debug_names() {
        assert_eq!(key_name(61), "AltGr");
        assert_eq!(key_name(54), "MetaRight");
        assert_eq!(key_name(53), "Escape");
        assert_eq!(key_name(0), "KeyA");
        assert_eq!(key_name(58), "Alt");
        assert_eq!(key_name(55), "MetaLeft");
        assert_eq!(key_name(59), "ControlLeft");
        assert_eq!(key_name(62), "ControlRight");
        assert_eq!(key_name(56), "ShiftLeft");
        assert_eq!(key_name(60), "ShiftRight");
        assert_eq!(key_name(63), "Function");
    }

    #[test]
    fn unknown_keycode_still_capturable() {
        assert_eq!(key_name(9999), "Unknown(9999)");
    }
}
