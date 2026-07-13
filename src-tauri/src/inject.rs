use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error("keystroke: {0}")]
    Keystroke(String),
}

pub fn copy_only(text: &str) -> Result<(), InjectError> {
    let mut cb = arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    cb.set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))
}

/// Saves the clipboard, puts `text` in it, simulates Cmd/Ctrl+V into the
/// frontmost app, then restores the original clipboard.
pub fn inject_text(text: &str) -> Result<(), InjectError> {
    let mut cb = arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    let previous = cb.get_text().ok();
    cb.set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))?;

    // Give the OS clipboard a beat before pasting.
    std::thread::sleep(Duration::from_millis(80));

    let result = paste_keystroke();

    // Let the paste land before restoring the old clipboard.
    std::thread::sleep(Duration::from_millis(150));
    if let Some(prev) = previous {
        let _ = cb.set_text(prev);
    }
    result
}

// enigo's macOS backend calls TISCopyCurrentKeyboardInputSource /
// UCKeyTranslate (HIToolbox/TSM) to synthesize keystrokes. On macOS 15,
// TSM asserts it's running on the main thread; `inject_text` runs on a
// tokio blocking thread, so that assert kills the process (EXC_BREAKPOINT)
// on every dictation. Post the Cmd+V keystroke directly via CGEvent instead
// — CGEventPost is thread-safe and never touches HIToolbox/TSM.
#[cfg(target_os = "macos")]
fn paste_keystroke() -> Result<(), InjectError> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KEY_V: core_graphics::event::CGKeyCode = 9;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InjectError::Keystroke("CGEventSourceCreate failed".to_string()))?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| InjectError::Keystroke("CGEventCreateKeyboardEvent (down) failed".to_string()))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    std::thread::sleep(Duration::from_millis(20));

    let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| InjectError::Keystroke("CGEventCreateKeyboardEvent (up) failed".to_string()))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn paste_keystroke() -> Result<(), InjectError> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| InjectError::Keystroke(e.to_string()))?;
    let modifier = Key::Control;

    // The modifier release must always be attempted, even if press or click
    // errors — otherwise a failed paste leaves Ctrl stuck down. Skip click
    // if press fails to avoid typing a bare 'v'. First error wins.
    let press_err = enigo.key(modifier, Direction::Press).err();
    let click_err = if press_err.is_none() {
        enigo.key(Key::Unicode('v'), Direction::Click).err()
    } else {
        None
    };
    let release_err = enigo.key(modifier, Direction::Release).err();
    match press_err.or(click_err).or(release_err) {
        Some(err) => Err(InjectError::Keystroke(err.to_string())),
        None => Ok(()),
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    #[test]
    fn cgevent_source_and_keyboard_event_construct() {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .expect("CGEventSourceCreate should succeed in a normal test environment");
        let event = CGEvent::new_keyboard_event(source, 9, true)
            .expect("CGEventCreateKeyboardEvent should succeed given a valid source");
        drop(event);
    }
}
