use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TAP_MS: u128 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    None,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Idle,
    /// Key held after initial down; recording is running.
    Ptt { down: u128 },
    /// Quick tap released; recording continues while we wait for a 2nd tap.
    TapArmed { up: u128 },
    /// Double-tap lock: recording until next key-down.
    Locked,
    /// Stop was emitted on key-down; swallow the matching key-up.
    Stopping,
}

pub struct Interpreter {
    mode: Mode,
}

impl Interpreter {
    pub fn new() -> Self {
        Self { mode: Mode::Idle }
    }

    /// Forces the interpreter back to Idle. Used when the pipeline moves to
    /// Idle for a reason the interpreter itself didn't decide (Esc cancel,
    /// the 300s auto-stop) — without this, the physical key state it tracks
    /// (e.g. Locked with no key held) would desync from the pipeline and eat
    /// the next real press.
    pub fn reset(&mut self) {
        self.mode = Mode::Idle;
    }

    pub fn key_down(&mut self, t: u128) -> Action {
        match self.mode {
            Mode::Idle => {
                self.mode = Mode::Ptt { down: t };
                Action::Start
            }
            Mode::TapArmed { .. } => {
                self.mode = Mode::Locked;
                Action::None
            }
            Mode::Locked => {
                self.mode = Mode::Stopping;
                Action::Stop
            }
            _ => Action::None,
        }
    }

    pub fn key_up(&mut self, t: u128) -> Action {
        match self.mode {
            Mode::Ptt { down } if t.saturating_sub(down) < TAP_MS => {
                self.mode = Mode::TapArmed { up: t };
                Action::None
            }
            Mode::Ptt { .. } => {
                self.mode = Mode::Idle;
                Action::Stop
            }
            Mode::Stopping => {
                self.mode = Mode::Idle;
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Call every ~50 ms; resolves an expired double-tap window.
    pub fn tick(&mut self, t: u128) -> Action {
        if let Mode::TapArmed { up } = self.mode {
            if t.saturating_sub(up) >= TAP_MS {
                self.mode = Mode::Idle;
                return Action::Stop;
            }
        }
        Action::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySignal {
    Start,
    Stop,
    Cancel,
}

pub fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

/// Spawns the global rdev listener + tick thread. Never returns handles —
/// both threads live for the app's lifetime. `on_dead` fires (once) if the
/// rdev listener fails to start, e.g. missing Accessibility permission.
/// `capture_next` is a one-shot flag: while true, the next KeyPress of ANY
/// key is diverted to `on_captured` (Settings' "Zmeniť" hotkey flow) instead
/// of being interpreted as the configured hotkey.
pub fn spawn(
    hotkey: Arc<RwLock<String>>,
    tx: mpsc::Sender<HotkeySignal>,
    capture_next: Arc<AtomicBool>,
    on_dead: Box<dyn Fn(String) + Send>,
    on_captured: Box<dyn Fn(String) + Send>,
) -> Arc<std::sync::Mutex<Interpreter>> {
    let interp = Arc::new(std::sync::Mutex::new(Interpreter::new()));

    {
        let interp = interp.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let tx_esc = tx.clone();
            let send = move |a: Action| match a {
                Action::Start => { let _ = tx.send(HotkeySignal::Start); }
                Action::Stop => { let _ = tx.send(HotkeySignal::Stop); }
                Action::None => {}
            };
            // Set once a capture-mode KeyPress is swallowed, holding that
            // key's name, so its matching KeyRelease is swallowed too —
            // otherwise that release could be misread as a key-up of the
            // (still) configured hotkey. Only the release of THIS key is
            // swallowed; releases of any other key (e.g. a hotkey the user
            // was already holding) still flow through to interpretation.
            let mut swallow_release_of: Option<String> = None;
            // Shared between both event sources below — only where the key
            // events come from (rdev vs. a raw CGEventTap) differs per OS;
            // once we have a (name, is_down) pair the interpretation is the
            // same everywhere.
            let mut handle_event = move |name: String, is_down: bool| {
                if is_down && capture_next.swap(false, Ordering::SeqCst) {
                    swallow_release_of = Some(name.clone());
                    if name != "Escape" {
                        on_captured(name);
                    }
                    return;
                }
                if !is_down && swallow_release_of.as_deref() == Some(name.as_str()) {
                    swallow_release_of = None;
                    return;
                }

                let target = hotkey.read().unwrap().clone();
                if name == target {
                    let t = now_ms();
                    let mut i = interp.lock().unwrap();
                    let a = if is_down { i.key_down(t) } else { i.key_up(t) };
                    send(a);
                } else if name == "Escape" && is_down {
                    // Only reached when Escape isn't itself the configured
                    // hotkey; pipeline ignores Cancel while Idle.
                    let _ = tx_esc.send(HotkeySignal::Cancel);
                }
            };

            // rdev's macOS listener calls into HIToolbox
            // (Keyboard::create_string_for_key) on every KeyPress, which
            // asserts main-thread on macOS 15 and crashes the app the first
            // time this thread sees a key. macos_tap.rs never touches
            // HIToolbox, so macOS gets its own event source; every other
            // platform keeps using rdev as before.
            // A session tap created WITHOUT the Accessibility grant does not
            // fail — it silently degrades to delivering only our own app's
            // events (hotkey "works" only while Dikto is focused). Never
            // create the tap in that state: warn, then wait for the grant —
            // TCC applies it live, so no app restart is needed.
            #[cfg(target_os = "macos")]
            let result = {
                use macos_accessibility_client::accessibility::application_is_trusted;
                if !application_is_trusted() {
                    on_dead(
                        "Globálna klávesa nefunguje — chýba povolenie Prístupnosť. \
                         Otvor Nastavenia → Súkromie a bezpečnosť → Prístupnosť a povoľ Dikto; \
                         appka to zachytí sama, netreba ju reštartovať."
                            .to_string(),
                    );
                    while !application_is_trusted() {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                    }
                }
                crate::macos_tap::listen(move |ev| {
                    let (name, is_down) = match ev {
                        crate::macos_tap::TapEvent::Down(n) => (n, true),
                        crate::macos_tap::TapEvent::Up(n) => (n, false),
                    };
                    handle_event(name, is_down);
                })
            };

            #[cfg(not(target_os = "macos"))]
            let result = rdev::listen(move |ev| {
                let key_name = match ev.event_type {
                    rdev::EventType::KeyPress(k) => Some((format!("{k:?}"), true)),
                    rdev::EventType::KeyRelease(k) => Some((format!("{k:?}"), false)),
                    _ => None,
                };
                let Some((name, is_down)) = key_name else { return };
                handle_event(name, is_down);
            });

            if let Err(e) = result {
                eprintln!("hotkey listener failed: {e:?}");
                #[cfg(target_os = "macos")]
                let message = "Globálna klávesa nefunguje — chýba povolenie Prístupnosť. \
                               Otvor Nastavenia → Súkromie a bezpečnosť → Prístupnosť."
                    .to_string();
                #[cfg(not(target_os = "macos"))]
                let message = format!(
                    "Globálna klávesa nefunguje — sledovanie klávesnice sa nepodarilo spustiť ({e:?}). \
                     Skús Dikto reštartovať."
                );
                on_dead(message);
            }
        });
    }

    // tick thread
    let interp_for_tick = interp.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let a = interp_for_tick.lock().unwrap().tick(now_ms());
        if a == Action::Stop {
            let _ = tx.send(HotkeySignal::Stop);
        }
    });

    interp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_is_push_to_talk() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.tick(200), Action::None);
        assert_eq!(i.key_up(500), Action::Stop);
    }

    #[test]
    fn double_tap_locks_and_third_press_stops() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.key_up(100), Action::None); // quick tap → armed
        assert_eq!(i.key_down(200), Action::None); // 2nd tap → locked
        assert_eq!(i.key_up(280), Action::None);
        assert_eq!(i.tick(1000), Action::None); // locked: no timeout stop
        assert_eq!(i.key_down(5000), Action::Stop); // 3rd press stops
        assert_eq!(i.key_up(5050), Action::None); // its release swallowed
    }

    #[test]
    fn single_quick_tap_stops_on_window_expiry() {
        let mut i = Interpreter::new();
        assert_eq!(i.key_down(0), Action::Start);
        assert_eq!(i.key_up(100), Action::None);
        assert_eq!(i.tick(350), Action::None); // 250 ms after up: still armed
        assert_eq!(i.tick(401), Action::Stop); // window expired
    }

    #[test]
    fn after_stop_cycle_can_start_again() {
        let mut i = Interpreter::new();
        i.key_down(0);
        i.key_up(500); // ptt stop
        assert_eq!(i.key_down(1000), Action::Start);
    }

    #[test]
    fn reset_from_locked_lets_next_key_down_start() {
        let mut i = Interpreter::new();
        i.key_down(0);
        i.key_up(100); // tap → armed
        i.key_down(200); // 2nd tap → locked
        i.key_up(280);
        i.reset(); // pipeline-side cancel/auto-stop while physically Locked
        assert_eq!(i.key_down(1000), Action::Start);
    }

    #[test]
    fn reset_from_tap_armed_suppresses_phantom_stop_on_tick() {
        let mut i = Interpreter::new();
        i.key_down(0);
        i.key_up(100); // tap → armed, waiting for a 2nd tap or window expiry
        i.reset();
        assert_eq!(i.tick(1000), Action::None);
    }
}
