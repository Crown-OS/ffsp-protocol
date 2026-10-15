use bitflags::bitflags;

bitflags! {
    /// What a peer can do, exchanged in discovery beacons and confirmed during the handshake.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Capabilities: u16 {
        const WIFI_INFRA = 1 << 0;
        const WIFI_DIRECT = 1 << 1;
        const BLUETOOTH_LE = 1 << 2;
        const BLUETOOTH_CLASSIC = 1 << 3;
        const USB_ACCESSORY = 1 << 4;
        /// Set when the peer has an AES instruction set, which decides the negotiated cipher.
        const AES_HARDWARE = 1 << 5;
    }
}

impl Capabilities {
    pub const fn from_bits_lossy(bits: u16) -> Self {
        Self::from_bits_truncate(bits)
    }

    /// AES-GCM is only chosen when both peers accelerate it; otherwise ChaCha20 is faster.
    pub const fn both_accelerate_aes(self, peer: Self) -> bool {
        self.contains(Self::AES_HARDWARE) && peer.contains(Self::AES_HARDWARE)
    }

    pub const fn shared_mediums(self, peer: Self) -> Self {
        self.intersection(peer)
    }
}
