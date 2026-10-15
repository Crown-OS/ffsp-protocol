use crate::discovery::advertisement::ADVERTISEMENT_LEN;
use crate::discovery::{Advertisement, DiscoveryError};
use crate::util::hex;

/// DNS-SD service type browsed and registered on the local network.
pub const SERVICE_TYPE: &str = "_crowncrate._tcp";

const BEACON_KEY: &str = "d";
const PORT_KEY: &str = "p";

/// The mDNS half of discovery.
///
/// The beacon is the byte-identical [`Advertisement`] the BLE plane carries, hex-encoded. Sharing
/// one payload means the two planes cannot drift, and the ephemeral identifier appearing in both
/// is what lets one physical device be recognised as one peer across them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxtRecord {
    pub advertisement: Advertisement,
    pub port: u16,
}

impl TxtRecord {
    pub fn to_pairs(&self) -> [(&'static str, String); 2] {
        [
            (BEACON_KEY, hex::encode(&self.advertisement.to_bytes())),
            (PORT_KEY, self.port.to_string()),
        ]
    }

    pub fn parse<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> Result<Self, DiscoveryError> {
        let beacon = lookup(BEACON_KEY).ok_or(DiscoveryError::MissingTxtKey(BEACON_KEY))?;
        let beacon = hex::decode::<ADVERTISEMENT_LEN>(beacon)
            .ok_or(DiscoveryError::MalformedTxtValue { key: BEACON_KEY })?;
        let port = lookup(PORT_KEY)
            .ok_or(DiscoveryError::MissingTxtKey(PORT_KEY))?
            .parse()
            .map_err(|_| DiscoveryError::MalformedTxtValue { key: PORT_KEY })?;
        Ok(Self {
            advertisement: Advertisement::parse(&beacon)?,
            port,
        })
    }
}
