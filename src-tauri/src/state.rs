use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Injecting,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    StartRequested,
    StopRequested,
    Cancel,
    TranscriptReady,
    CleanupDone,
    Injected,
    Failed,
    RetryRequested,
}

/// Returns the next phase, or None when the event is illegal in this phase
/// (caller ignores it — e.g. a stray key-up while Idle).
pub fn transition(p: Phase, e: Event) -> Option<Phase> {
    use Event::*;
    use Phase::*;
    match (p, e) {
        (Idle, StartRequested) => Some(Recording),
        (Error, StartRequested) => Some(Recording),
        (Recording, StopRequested) => Some(Transcribing),
        (Recording, Cancel) => Some(Idle),
        (Transcribing, TranscriptReady) => Some(Cleaning),
        (Transcribing, Cancel) => Some(Idle),
        (Cleaning, CleanupDone) => Some(Injecting),
        (Cleaning, Cancel) => Some(Idle),
        (Injecting, Injected) => Some(Idle),
        (_, Failed) => Some(Error),
        (Error, Cancel) => Some(Idle),
        (Error, RetryRequested) => Some(Transcribing),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;
    use Phase::*;

    #[test]
    fn happy_path() {
        let mut p = Idle;
        for (e, want) in [
            (StartRequested, Recording),
            (StopRequested, Transcribing),
            (TranscriptReady, Cleaning),
            (CleanupDone, Injecting),
            (Injected, Idle),
        ] {
            p = transition(p, e).unwrap();
            assert_eq!(p, want);
        }
    }

    #[test]
    fn cancel_returns_to_idle_from_active_phases() {
        for p in [Recording, Transcribing, Cleaning, Error] {
            assert_eq!(transition(p, Cancel), Some(Idle));
        }
    }

    #[test]
    fn failure_from_any_phase_goes_to_error() {
        for p in [Recording, Transcribing, Cleaning, Injecting] {
            assert_eq!(transition(p, Failed), Some(Error));
        }
    }

    #[test]
    fn error_can_restart() {
        assert_eq!(transition(Error, StartRequested), Some(Recording));
    }

    #[test]
    fn retry_legal_from_error() {
        assert_eq!(transition(Error, RetryRequested), Some(Transcribing));
    }

    #[test]
    fn retry_illegal_outside_error() {
        assert_eq!(transition(Idle, RetryRequested), None);
        assert_eq!(transition(Recording, RetryRequested), None);
    }

    #[test]
    fn illegal_events_are_none() {
        assert_eq!(transition(Idle, StopRequested), None);
        assert_eq!(transition(Idle, Injected), None);
        assert_eq!(transition(Recording, StartRequested), None);
    }

    #[test]
    fn phase_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Transcribing).unwrap(), "\"transcribing\"");
    }
}
