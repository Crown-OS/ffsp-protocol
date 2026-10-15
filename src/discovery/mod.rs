//! Finding linked devices that are nearby, without telling anyone else who is here.

mod advertisement;
mod ephemeral_id;
mod epoch;
mod error;

mod presence;
mod resolver;
mod txt_record;

pub use advertisement::{ADVERTISEMENT_LEN, Advertisement, SERVICE_UUID_16};
pub use ephemeral_id::{EphemeralId, ephemeral_id};
pub use epoch::{EPOCH_SECONDS, Epoch};
pub use error::DiscoveryError;
pub use presence::{MediumSighting, NearbyPeer, PresenceChange, PresenceTracker, Sighting};
pub use resolver::PresenceResolver;
pub use txt_record::{SERVICE_TYPE, TxtRecord};

#[cfg(test)]
mod tests;
