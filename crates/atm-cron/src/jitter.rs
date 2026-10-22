use chrono::{DateTime, Utc};

pub trait JitterSource: Send + Sync {
    fn below(&self, upper: u32) -> u32;
}

pub struct SystemJitter;

impl JitterSource for SystemJitter {
    fn below(&self, upper: u32) -> u32 {
        assert!(upper > 0);
        let threshold = upper.wrapping_neg() % upper;
        loop {
            let mut bytes = [0; 4];
            getrandom::fill(&mut bytes).expect("operating system random source unavailable");
            let value = u32::from_ne_bytes(bytes);
            if value >= threshold {
                return value % upper;
            }
        }
    }
}

#[cfg(feature = "test-utils")]
#[derive(Default)]
pub struct SeqJitter {
    pub values: std::sync::Mutex<std::collections::VecDeque<u32>>,
    pub calls: std::sync::Mutex<Vec<u32>>,
}

#[cfg(feature = "test-utils")]
impl JitterSource for SeqJitter {
    fn below(&self, upper: u32) -> u32 {
        self.calls
            .lock()
            .expect("jitter calls poisoned")
            .push(upper);
        let value = self
            .values
            .lock()
            .expect("jitter values poisoned")
            .pop_front()
            .unwrap_or(0);
        assert!(value < upper, "controlled jitter out of range");
        value
    }
}

pub(crate) fn assign(
    schedule: &cron::Schedule,
    now: DateTime<Utc>,
    held: &[u32],
    source: &dyn JitterSource,
) -> u32 {
    let mut occurrences = schedule.after(&now);
    let first = occurrences.next();
    let second = occurrences.next();
    let upper = match (first, second) {
        (Some(first), Some(second)) => ((second - first).num_seconds() / 2).clamp(1, 1800) as u32,
        _ => 1,
    };
    let free: Vec<_> = (0..upper).filter(|offset| !held.contains(offset)).collect();
    if free.is_empty() {
        source.below(upper)
    } else {
        free[source.below(free.len() as u32) as usize]
    }
}
