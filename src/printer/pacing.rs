//! Validated timing and retry policy for print jobs.

use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

/// Timing and retry budget for a print job.
///
/// Byte and retry budgets use non-zero integer types, so a [`Pacing`] value
/// cannot describe a print loop that makes no progress. Real jobs and tests
/// run the same control flow; only these values differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pacing {
    row_pause: Duration,
    pace_bytes: NonZeroUsize,
    after_page_end: Duration,
    between_polls: Duration,
    poll_wait: Duration,
    end_print_tries: NonZeroU32,
    between_end_tries: Duration,
}

impl Default for Pacing {
    fn default() -> Self {
        Self::REAL
    }
}

impl Pacing {
    /// Timings tuned against real B1 hardware.
    pub const REAL: Self = Self {
        row_pause: Duration::from_millis(5),
        pace_bytes: NonZeroUsize::new(64).unwrap(),
        after_page_end: Duration::from_millis(200),
        between_polls: Duration::from_millis(50),
        poll_wait: Duration::from_millis(100),
        end_print_tries: NonZeroU32::new(50).unwrap(),
        between_end_tries: Duration::from_millis(100),
    };

    /// Slower diagnostic pacing, via `THERMARK_SLOW=1`.
    pub const CAREFUL: Self = Self {
        row_pause: Duration::from_millis(32),
        pace_bytes: NonZeroUsize::new(256).unwrap(),
        after_page_end: Duration::from_millis(300),
        between_polls: Duration::from_millis(50),
        poll_wait: Duration::from_millis(100),
        end_print_tries: NonZeroU32::new(50).unwrap(),
        between_end_tries: Duration::from_millis(100),
    };

    /// Same command sequence and retry counts without real-time waits.
    pub const INSTANT: Self = Self {
        row_pause: Duration::ZERO,
        pace_bytes: NonZeroUsize::new(256).unwrap(),
        after_page_end: Duration::ZERO,
        between_polls: Duration::ZERO,
        poll_wait: Duration::from_millis(1),
        end_print_tries: NonZeroU32::new(50).unwrap(),
        between_end_tries: Duration::ZERO,
    };

    pub const fn row_pause(self) -> Duration {
        self.row_pause
    }

    pub const fn pace_bytes(self) -> NonZeroUsize {
        self.pace_bytes
    }

    pub const fn after_page_end(self) -> Duration {
        self.after_page_end
    }

    pub const fn between_polls(self) -> Duration {
        self.between_polls
    }

    pub const fn poll_wait(self) -> Duration {
        self.poll_wait
    }

    pub const fn end_print_tries(self) -> NonZeroU32 {
        self.end_print_tries
    }

    pub const fn between_end_tries(self) -> Duration {
        self.between_end_tries
    }

    pub const fn with_row_pause(mut self, value: Duration) -> Self {
        self.row_pause = value;
        self
    }

    pub const fn with_pace_bytes(mut self, value: NonZeroUsize) -> Self {
        self.pace_bytes = value;
        self
    }

    pub const fn with_after_page_end(mut self, value: Duration) -> Self {
        self.after_page_end = value;
        self
    }

    pub const fn with_between_polls(mut self, value: Duration) -> Self {
        self.between_polls = value;
        self
    }

    pub const fn with_poll_wait(mut self, value: Duration) -> Self {
        self.poll_wait = value;
        self
    }

    pub const fn with_end_print_tries(mut self, value: NonZeroU32) -> Self {
        self.end_print_tries = value;
        self
    }

    pub const fn with_between_end_tries(mut self, value: Duration) -> Self {
        self.between_end_tries = value;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customization_preserves_non_zero_budgets() {
        let pacing = Pacing::REAL
            .with_pace_bytes(NonZeroUsize::new(1).unwrap())
            .with_end_print_tries(NonZeroU32::new(1).unwrap());
        assert_eq!(pacing.pace_bytes().get(), 1);
        assert_eq!(pacing.end_print_tries().get(), 1);
        assert!(NonZeroUsize::new(0).is_none());
        assert!(NonZeroU32::new(0).is_none());
    }
}
