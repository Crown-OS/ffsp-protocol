//! What the engine tells the platform, what the platform tells it, and the loop between.
//!
//! Discovery and connection are the platform's: only it knows what BlueZ, `UsbManager` or
//! NetworkManager want. The engine is handed lanes that already work, and decides what crosses
//! them. Events are pulled rather than pushed, so no task here ever has to enter the platform's
//! runtime.

mod command;
mod config;
mod error;
mod event;
mod handle;
mod handshake;
mod inbound;
mod outbound;
mod peer;
mod runtime;
mod tuning;

pub use command::{AdoptedLane, EngineCommand};
pub use config::EngineConfig;
pub use error::EngineError;
pub use event::{Diagnostic, EngineEvent, LaneProgress, TransferOutcome};
pub use handle::{Engine, EngineEvents};
pub use tuning::Tuning;

use tokio::sync::mpsc;

/// Starts the engine on the current runtime.
pub fn start(config: EngineConfig) -> (Engine, EngineEvents) {
    let (commands_tx, commands_rx) = mpsc::unbounded_channel();
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let runtime = runtime::Runtime::new(config, events_tx);
    tokio::spawn(runtime.run(commands_rx));
    (Engine::new(commands_tx), EngineEvents::new(events_rx))
}

#[cfg(test)]
mod tests;
