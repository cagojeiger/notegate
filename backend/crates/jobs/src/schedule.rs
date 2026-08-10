use std::time::Duration;

use uuid::Uuid;

const NON_ZERO_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

pub(crate) struct Jitter {
    state: u64,
}

impl Jitter {
    pub(crate) fn random() -> Self {
        let [a, b, c, d, e, f, g, h, ..] = Uuid::new_v4().into_bytes();
        Self::from_seed(u64::from_le_bytes([a, b, c, d, e, f, g, h]))
    }

    pub(crate) fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { NON_ZERO_SEED } else { seed },
        }
    }

    pub(crate) fn symmetric(&mut self, base: Duration, percent: u32) -> Duration {
        let percent = percent.min(100);
        between(base, 100 - percent, 100 + percent, self.next())
    }

    pub(crate) fn spread(&mut self, maximum: Duration) -> Duration {
        between(maximum, 0, 100, self.next())
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

pub(crate) fn job_entropy(job_id: Uuid, attempt: i32) -> u64 {
    let [a, b, c, d, e, f, g, h, ..] = job_id.into_bytes();
    u64::from_le_bytes([a, b, c, d, e, f, g, h]) ^ u64::try_from(attempt).unwrap_or_default()
}

pub(crate) fn between(
    base: Duration,
    minimum_percent: u32,
    maximum_percent: u32,
    entropy: u64,
) -> Duration {
    debug_assert!(minimum_percent <= maximum_percent);
    let width = u64::from(maximum_percent - minimum_percent) + 1;
    let percent = u64::from(minimum_percent) + entropy % width;
    let nanoseconds = base
        .as_nanos()
        .saturating_mul(u128::from(percent))
        .saturating_div(100)
        .min(Duration::MAX.as_nanos());
    Duration::new(
        u64::try_from(nanoseconds / 1_000_000_000).unwrap_or(u64::MAX),
        u32::try_from(nanoseconds % 1_000_000_000).unwrap_or(999_999_999),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_jitter_stays_inside_the_requested_range() {
        let base = Duration::from_secs(100);
        assert_eq!(between(base, 90, 110, 0), Duration::from_secs(90));
        assert_eq!(between(base, 90, 110, 20), Duration::from_secs(110));
    }

    #[test]
    fn positive_jitter_never_runs_before_the_base_delay() {
        let base = Duration::from_secs(100);
        for entropy in 0..100 {
            let delay = between(base, 100, 120, entropy);
            assert!((base..=Duration::from_secs(120)).contains(&delay));
        }
    }

    #[test]
    fn wake_spread_stays_bounded() {
        let maximum = Duration::from_millis(50);
        let mut jitter = Jitter::from_seed(2);
        for _ in 0..100 {
            assert!(jitter.spread(maximum) <= maximum);
        }
    }

    #[test]
    fn jitter_saturates_instead_of_overflowing() {
        assert_eq!(between(Duration::MAX, 120, 120, 0), Duration::MAX);
    }
}
