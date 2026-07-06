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
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut cb = arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    let previous = cb.get_text().ok();
    cb.set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))?;

    // Give the OS clipboard a beat before pasting.
    std::thread::sleep(Duration::from_millis(80));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| InjectError::Keystroke(e.to_string()))?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let press = |e: &mut Enigo| -> Result<(), InjectError> {
        e.key(modifier, Direction::Press)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        e.key(Key::Unicode('v'), Direction::Click)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        e.key(modifier, Direction::Release)
            .map_err(|err| InjectError::Keystroke(err.to_string()))?;
        Ok(())
    };
    let result = press(&mut enigo);

    // Let the paste land before restoring the old clipboard.
    std::thread::sleep(Duration::from_millis(150));
    if let Some(prev) = previous {
        let _ = cb.set_text(prev);
    }
    result
}
