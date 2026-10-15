use crate::control::error::ControlError;
use crate::control::manifest_codec::truncated;
use crate::medium::Medium;
use crate::util::cursor::{ByteReader, write_string};
use crate::wire::LaneId;

/// An offer to bring up another medium, and the identifier the new lane will answer to.
///
/// The identifier travels because both ends key their ciphers on it
/// ([`SessionSecret::lane_key`](crate::crypto::SessionSecret::lane_key) mixes it into the KDF) and
/// because neither end can predict the other's bring-up order: USB arrives when the cable is
/// plugged, Wi-Fi when discovery resolves. Letting each side number its own lanes would have them
/// agree only by luck, and disagree by decrypting under the wrong key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneInvite {
    /// Chosen by the session initiator, which is the only allocator.
    pub lane: LaneId,
    pub medium: Medium,
    /// How to reach the inviter over this medium: a host and port, a Bluetooth address, a USB
    /// path. Free-form, because only the driver for that medium has to understand it.
    pub endpoint: String,
}

impl LaneInvite {
    pub(crate) fn encode(&self, out: &mut Vec<u8>) -> Result<(), ControlError> {
        out.extend_from_slice(&self.lane.as_u16().to_le_bytes());
        out.push(self.medium.as_byte());
        write_string(out, &self.endpoint).ok_or(ControlError::FieldTooLong)
    }

    pub(crate) fn decode(reader: &mut ByteReader<'_>) -> Result<Self, ControlError> {
        Ok(Self {
            lane: LaneId::new(reader.u16().ok_or(truncated(reader))?),
            medium: Medium::from_byte(reader.u8().ok_or(truncated(reader))?)?,
            endpoint: reader.string().ok_or(truncated(reader))?,
        })
    }
}
