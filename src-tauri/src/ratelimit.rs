use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Groq's free tier allows 20 requests/minute for whisper; we aim under it so
/// the final transcription never races the preview loop for the last slot.
pub const DEFAULT_RPM: u32 = 18;
/// Slots held back from the live-preview loop. Once fewer than this many
/// remain in the current minute, only final transcriptions get through.
pub const DEFAULT_PARTIAL_RESERVE: u32 = 6;

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// The transcription the user is waiting for. Always admitted — the STT
    /// client's own backoff deals with the API pushing back.
    Final,
    /// The live preview during recording. Expendable by design.
    Partial,
}

struct Inner {
    hits: VecDeque<Instant>,
    cooldown_until: Option<Instant>,
}

/// Client-side throttle in front of Groq, shared by the preview loop and the
/// final transcription. Its whole job is making sure a burst of previews can
/// never be the reason the take the user actually cares about gets a 429.
pub struct Limiter {
    rpm: u32,
    partial_reserve: u32,
    inner: Mutex<Inner>,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(DEFAULT_RPM, DEFAULT_PARTIAL_RESERVE)
    }
}

impl Limiter {
    pub fn new(rpm: u32, partial_reserve: u32) -> Self {
        Self {
            rpm,
            partial_reserve,
            inner: Mutex::new(Inner {
                hits: VecDeque::new(),
                cooldown_until: None,
            }),
        }
    }

    pub fn try_acquire(&self, priority: Priority) -> bool {
        self.try_acquire_at(priority, Instant::now())
    }

    pub fn try_acquire_at(&self, priority: Priority, now: Instant) -> bool {
        let mut inner = self.inner.lock().unwrap();
        while inner.hits.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
            inner.hits.pop_front();
        }
        if inner.cooldown_until.is_some_and(|until| until <= now) {
            inner.cooldown_until = None;
        }
        if priority == Priority::Partial {
            if inner.cooldown_until.is_some() {
                return false;
            }
            let budget = self.rpm.saturating_sub(self.partial_reserve);
            if inner.hits.len() as u32 >= budget {
                return false;
            }
        }
        inner.hits.push_back(now);
        true
    }

    /// Records that Groq pushed back, so previews stand down until the window
    /// it asked for (or a default) has passed.
    pub fn note_rate_limited(&self, retry_after: Option<Duration>) {
        self.note_rate_limited_at(retry_after, Instant::now());
    }

    pub fn note_rate_limited_at(&self, retry_after: Option<Duration>, now: Instant) {
        let wait = retry_after.unwrap_or(Duration::from_secs(30)).min(WINDOW);
        let mut inner = self.inner.lock().unwrap();
        let until = now + wait;
        if inner.cooldown_until.is_none_or(|prev| prev < until) {
            inner.cooldown_until = Some(until);
        }
    }

    #[cfg(test)]
    fn cooling_down_at(&self, now: Instant) -> bool {
        self.inner
            .lock()
            .unwrap()
            .cooldown_until
            .is_some_and(|until| until > now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn partials_stop_once_the_reserve_is_all_that_is_left() {
        let l = Limiter::new(10, 4);
        let now = t0();
        // Budget for partials is rpm - reserve = 6.
        for i in 0..6 {
            assert!(l.try_acquire_at(Priority::Partial, now), "partial {i} should pass");
        }
        assert!(!l.try_acquire_at(Priority::Partial, now));
    }

    #[test]
    fn final_still_gets_through_when_partials_are_shut_out() {
        let l = Limiter::new(10, 4);
        let now = t0();
        for _ in 0..6 {
            l.try_acquire_at(Priority::Partial, now);
        }
        assert!(!l.try_acquire_at(Priority::Partial, now));
        assert!(l.try_acquire_at(Priority::Final, now));
    }

    #[test]
    fn final_is_admitted_even_past_the_nominal_rpm() {
        let l = Limiter::new(3, 1);
        let now = t0();
        for _ in 0..10 {
            assert!(l.try_acquire_at(Priority::Final, now));
        }
    }

    #[test]
    fn the_window_slides_so_partials_recover_after_a_minute() {
        let l = Limiter::new(10, 4);
        let now = t0();
        for _ in 0..6 {
            l.try_acquire_at(Priority::Partial, now);
        }
        assert!(!l.try_acquire_at(Priority::Partial, now));
        assert!(l.try_acquire_at(Priority::Partial, now + Duration::from_secs(61)));
    }

    #[test]
    fn a_429_benches_partials_for_the_requested_window() {
        let l = Limiter::new(100, 1);
        let now = t0();
        assert!(l.try_acquire_at(Priority::Partial, now));
        l.note_rate_limited_at(Some(Duration::from_secs(10)), now);

        assert!(!l.try_acquire_at(Priority::Partial, now + Duration::from_secs(5)));
        assert!(l.try_acquire_at(Priority::Final, now + Duration::from_secs(5)));
        assert!(l.try_acquire_at(Priority::Partial, now + Duration::from_secs(11)));
    }

    #[test]
    fn cooldown_defaults_when_the_api_gives_no_retry_after() {
        let l = Limiter::new(100, 1);
        let now = t0();
        l.note_rate_limited_at(None, now);
        assert!(l.cooling_down_at(now + Duration::from_secs(29)));
        assert!(!l.cooling_down_at(now + Duration::from_secs(31)));
    }

    #[test]
    fn an_absurd_retry_after_is_clamped_to_the_window() {
        let l = Limiter::new(100, 1);
        let now = t0();
        l.note_rate_limited_at(Some(Duration::from_secs(3600)), now);
        assert!(!l.cooling_down_at(now + Duration::from_secs(61)));
    }

    #[test]
    fn a_longer_cooldown_wins_over_a_shorter_one_still_running() {
        let l = Limiter::new(100, 1);
        let now = t0();
        l.note_rate_limited_at(Some(Duration::from_secs(30)), now);
        l.note_rate_limited_at(Some(Duration::from_secs(2)), now);
        assert!(l.cooling_down_at(now + Duration::from_secs(20)));
    }
}
