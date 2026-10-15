use tokio::sync::mpsc;

use crate::engine::{EngineCommand, EngineError, EngineEvent};

/// Sends the engine work. Cloneable, so a UI and a discovery loop can both hold one.
#[derive(Debug, Clone)]
pub struct Engine {
    commands: mpsc::UnboundedSender<EngineCommand>,
}

impl Engine {
    pub(crate) const fn new(commands: mpsc::UnboundedSender<EngineCommand>) -> Self {
        Self { commands }
    }

    pub fn send(&self, command: EngineCommand) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::Stopped)
    }

    pub fn shutdown(&self) -> Result<(), EngineError> {
        self.send(EngineCommand::Shutdown)
    }
}

/// Where the engine's events come out.
///
/// Pulled rather than delivered through a callback, so no task of this crate ever has to enter
/// the platform's runtime: a Kotlin thread blocks on `next`, and a xilem worker awaits it.
#[derive(Debug)]
pub struct EngineEvents {
    events: mpsc::UnboundedReceiver<EngineEvent>,
}

impl EngineEvents {
    pub(crate) const fn new(events: mpsc::UnboundedReceiver<EngineEvent>) -> Self {
        Self { events }
    }

    pub async fn next(&mut self) -> Option<EngineEvent> {
        self.events.recv().await
    }

    /// Takes an event if one is waiting, for a caller that cannot await.
    pub fn try_next(&mut self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }
}
