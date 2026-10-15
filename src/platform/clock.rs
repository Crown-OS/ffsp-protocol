use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Monotonic time, injected so the scheduler is a pure function of it.
///
/// Every timing decision in [`transfer`](crate::transfer) reads the clock through this trait, which
/// is what lets a multi-second striping scenario run deterministically in microseconds.
pub trait Clock: Send + Sync + core::fmt::Debug {
    fn now_millis(&self) -> u64;

    /// Monotonic microseconds, for measurements a millisecond cannot resolve: a megabyte over
    /// USB takes about thirty of them.
    fn now_micros(&self) -> u64 {
        self.now_millis().saturating_mul(1_000)
    }

    /// Wall-clock seconds, used only to derive the discovery epoch.
    fn unix_seconds(&self) -> u64;
}

/// Fixed once, so monotonic readings survive the wall clock being corrected mid-transfer.
static STARTED_AT: LazyLock<Instant> = LazyLock::new(Instant::now);

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    /// Measured from an arbitrary fixed point rather than the epoch.
    ///
    /// Stall deadlines are differences of this, and a clock the system may step backwards would
    /// make a transfer either stall instantly or never.
    fn now_millis(&self) -> u64 {
        u64::try_from(STARTED_AT.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn now_micros(&self) -> u64 {
        u64::try_from(STARTED_AT.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn unix_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default()
    }
}

/// A clock tests advance by hand.
#[derive(Debug, Default)]
pub struct SimulatedClock {
    millis: AtomicU64,
}

impl SimulatedClock {
    pub const fn new(millis: u64) -> Self {
        Self {
            millis: AtomicU64::new(millis),
        }
    }

    pub fn advance(&self, millis: u64) {
        self.millis.fetch_add(millis, Ordering::Relaxed);
    }

    pub fn set(&self, millis: u64) {
        self.millis.store(millis, Ordering::Relaxed);
    }
}

impl Clock for SimulatedClock {
    fn now_millis(&self) -> u64 {
        self.millis.load(Ordering::Relaxed)
    }

    fn now_micros(&self) -> u64 {
        self.now_millis().saturating_mul(1_000)
    }

    fn unix_seconds(&self) -> u64 {
        self.now_millis() / 1_000
    }
}
