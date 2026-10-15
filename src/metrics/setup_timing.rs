use std::borrow::Cow;

/// One named stretch of the setup, measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub label: Cow<'static, str>,
    pub started_micros: u64,
    pub ended_micros: Option<u64>,
}

impl Phase {
    pub const fn elapsed_micros(&self) -> Option<u64> {
        match self.ended_micros {
            Some(ended) => Some(ended.saturating_sub(self.started_micros)),
            None => None,
        }
    }
}

/// How long each part of getting ready took.
///
/// Bringing three mediums up is most of the wall clock on a short transfer, and a single
/// "handshake duration" hides which of pairing, accessory switching or waiting for the
/// verification tree actually cost the time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupTiming {
    phases: Vec<Phase>,
}

impl SetupTiming {
    pub const fn new() -> Self {
        Self { phases: Vec::new() }
    }

    pub fn begin(&mut self, label: impl Into<Cow<'static, str>>, at_micros: u64) {
        self.phases.push(Phase {
            label: label.into(),
            started_micros: at_micros,
            ended_micros: None,
        });
    }

    /// Closes the most recent open phase of this name.
    pub fn end(&mut self, label: &str, at_micros: u64) {
        if let Some(phase) = self
            .phases
            .iter_mut()
            .rev()
            .find(|phase| phase.label == label && phase.ended_micros.is_none())
        {
            phase.ended_micros = Some(at_micros);
        }
    }

    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    pub fn elapsed_micros(&self, label: &str) -> Option<u64> {
        self.phases
            .iter()
            .find(|phase| phase.label == label)
            .and_then(Phase::elapsed_micros)
    }
}
