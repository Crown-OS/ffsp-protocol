use std::path::PathBuf;
use std::sync::Arc;

use crate::device::{Capabilities, DeviceClass};
use crate::engine::Tuning;
use crate::link::{LinkStore, StaticIdentity};
use crate::platform::{Clock, SystemClock};

/// Everything the engine needs from the platform that it cannot work out for itself.
#[derive(Clone)]
pub struct EngineConfig {
    pub identity: StaticIdentity,
    pub device_name: String,
    pub device_class: DeviceClass,
    pub capabilities: Capabilities,
    pub tuning: Tuning,
    pub clock: Arc<dyn Clock>,
    pub links: Arc<dyn LinkStore>,
    /// Where received files are written.
    ///
    /// This has to be somewhere positional writes work. Out-of-order arrival is what makes
    /// striping free, and a destination that only accepts sequential writes — an Android
    /// document provider handing back a pipe, say — turns three lanes back into one.
    pub destination: PathBuf,
    /// How often throughput is sampled for the timeline.
    pub sample_interval_millis: u64,
    /// How often each lane is pinged to measure its own round trip.
    pub keepalive_interval_millis: u64,
}

impl EngineConfig {
    pub const DEFAULT_SAMPLE_INTERVAL_MILLIS: u64 = 100;
    pub const DEFAULT_KEEPALIVE_INTERVAL_MILLIS: u64 = 1_000;

    pub fn new(identity: StaticIdentity, links: Arc<dyn LinkStore>, destination: PathBuf) -> Self {
        Self {
            identity,
            device_name: String::new(),
            device_class: DeviceClass::Laptop,
            capabilities: Capabilities::empty(),
            tuning: Tuning::default(),
            clock: Arc::new(SystemClock),
            links,
            destination,
            sample_interval_millis: Self::DEFAULT_SAMPLE_INTERVAL_MILLIS,
            keepalive_interval_millis: Self::DEFAULT_KEEPALIVE_INTERVAL_MILLIS,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = name.into();
        self
    }

    #[must_use]
    pub const fn with_class(mut self, class: DeviceClass) -> Self {
        self.device_class = class;
        self
    }

    #[must_use]
    pub const fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    #[must_use]
    pub const fn with_tuning(mut self, tuning: Tuning) -> Self {
        self.tuning = tuning;
        self
    }

    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }
}

impl core::fmt::Debug for EngineConfig {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("EngineConfig")
            .field("device", &self.identity.device_id())
            .field("name", &self.device_name)
            .field("class", &self.device_class)
            .field("capabilities", &self.capabilities)
            .field("tuning", &self.tuning)
            .field("destination", &self.destination)
            .finish_non_exhaustive()
    }
}
