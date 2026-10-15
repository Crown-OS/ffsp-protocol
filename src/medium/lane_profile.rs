use bitflags::bitflags;

use crate::medium::Medium;

/// The radio or bus a lane contends for.
///
/// Lanes sharing a group share airtime, so the scheduler never gives two of them bulk work at
/// once. This is what stops Wi-Fi Direct from cannibalising infrastructure Wi-Fi, and stops
/// Bluetooth from poisoning 2.4 GHz Wi-Fi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RadioGroup {
    WifiChipset,
    BluetoothChipset,
    Usb,
    Loopback,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LaneFlags: u8 {
        const CONTROL_CAPABLE = 1 << 0;
        const BULK_CAPABLE = 1 << 1;
        /// The user may be paying for these bytes, so bulk work avoids the lane by default.
        const METERED = 1 << 2;
    }
}

/// Largest single transfer the Android AOA driver reliably accepts.
pub const AOA_MAX_TRANSFER: usize = 16 * 1024;

/// Largest single write an RFCOMM socket accepts without the stack fragmenting behind our back.
pub const RFCOMM_MAX_TRANSFER: usize = 64 * 1024;

/// The frame ceiling for any lane that carries bulk chunks.
///
/// A bulk chunk is [`CHUNK_LEN`](crate::transfer::CHUNK_LEN) plus its index and AEAD tag, so a
/// lane whose frame limit sat below that could never carry one — which is why this is uniform
/// across bulk-capable mediums and the per-transfer limits below are what differ.
pub const BULK_FRAME_CEILING: usize = 4 * 1024 * 1024;

/// What to expect from a lane before anything has been measured.
///
/// The hints are priors only; [`LaneEstimator`](crate::transfer::LaneEstimator) replaces them with
/// observed values as soon as data flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneProfile {
    pub medium: Medium,
    pub radio_group: RadioGroup,
    /// Largest whole frame the lane will carry, header and tag included.
    pub max_frame_len: usize,
    /// Largest single read or write the transport underneath accepts.
    ///
    /// This is a property of the bus, not of the protocol: a USB bulk pipe takes 16 KiB at a time
    /// and an RFCOMM socket 64 KiB, but both carry a 1 MiB frame perfectly well by taking several
    /// turns over it, exactly as TCP segments a large write. Conflating the two would leave USB
    /// nominally bulk-capable and unable to carry a single chunk.
    pub max_transfer_len: usize,
    pub bandwidth_hint_kbps: u32,
    pub rtt_hint_micros: u32,
    pub flags: LaneFlags,
}

impl LaneProfile {
    pub const LOOPBACK: Self = Self {
        medium: Medium::Loopback,
        radio_group: RadioGroup::Loopback,
        max_frame_len: BULK_FRAME_CEILING,
        max_transfer_len: BULK_FRAME_CEILING,
        bandwidth_hint_kbps: 8_000_000,
        rtt_hint_micros: 10,
        flags: LaneFlags::CONTROL_CAPABLE.union(LaneFlags::BULK_CAPABLE),
    };

    pub const WIFI_INFRA: Self = Self {
        medium: Medium::WifiInfra,
        radio_group: RadioGroup::WifiChipset,
        max_frame_len: BULK_FRAME_CEILING,
        max_transfer_len: BULK_FRAME_CEILING,
        bandwidth_hint_kbps: 400_000,
        rtt_hint_micros: 4_000,
        flags: LaneFlags::CONTROL_CAPABLE.union(LaneFlags::BULK_CAPABLE),
    };

    pub const WIFI_DIRECT: Self = Self {
        medium: Medium::WifiDirect,
        radio_group: RadioGroup::WifiChipset,
        max_frame_len: BULK_FRAME_CEILING,
        max_transfer_len: BULK_FRAME_CEILING,
        bandwidth_hint_kbps: 240_000,
        rtt_hint_micros: 6_000,
        flags: LaneFlags::CONTROL_CAPABLE.union(LaneFlags::BULK_CAPABLE),
    };

    pub const USB_ACCESSORY: Self = Self {
        medium: Medium::UsbAccessory,
        radio_group: RadioGroup::Usb,
        max_frame_len: BULK_FRAME_CEILING,
        max_transfer_len: AOA_MAX_TRANSFER,
        bandwidth_hint_kbps: 280_000,
        rtt_hint_micros: 500,
        flags: LaneFlags::CONTROL_CAPABLE.union(LaneFlags::BULK_CAPABLE),
    };

    /// Control only: a 24-byte beacon budget and credit-limited throughput make bulk pointless.
    pub const BLUETOOTH_LE: Self = Self {
        medium: Medium::BluetoothLe,
        radio_group: RadioGroup::BluetoothChipset,
        max_frame_len: 4 * 1024,
        max_transfer_len: 4 * 1024,
        bandwidth_hint_kbps: 400,
        rtt_hint_micros: 60_000,
        flags: LaneFlags::CONTROL_CAPABLE,
    };

    /// Bulk capable, and slow enough that saying so is the point: a benchmark that excluded
    /// Bluetooth could never report what Bluetooth contributes. The scheduler's
    /// earliest-completion rule gives it a share proportional to what it can actually carry.
    pub const BLUETOOTH_CLASSIC: Self = Self {
        medium: Medium::BluetoothClassic,
        radio_group: RadioGroup::BluetoothChipset,
        max_frame_len: BULK_FRAME_CEILING,
        max_transfer_len: RFCOMM_MAX_TRANSFER,
        bandwidth_hint_kbps: 1_600,
        rtt_hint_micros: 30_000,
        flags: LaneFlags::CONTROL_CAPABLE.union(LaneFlags::BULK_CAPABLE),
    };

    pub const fn of(medium: Medium) -> Self {
        match medium {
            Medium::Loopback => Self::LOOPBACK,
            Medium::WifiInfra => Self::WIFI_INFRA,
            Medium::WifiDirect => Self::WIFI_DIRECT,
            Medium::UsbAccessory => Self::USB_ACCESSORY,
            Medium::BluetoothLe => Self::BLUETOOTH_LE,
            Medium::BluetoothClassic => Self::BLUETOOTH_CLASSIC,
        }
    }

    pub const fn carries_bulk(&self) -> bool {
        self.flags.contains(LaneFlags::BULK_CAPABLE)
    }

    pub const fn carries_control(&self) -> bool {
        self.flags.contains(LaneFlags::CONTROL_CAPABLE)
    }

    pub const fn contends_with(&self, other: &Self) -> bool {
        !matches!(self.radio_group, RadioGroup::Loopback)
            && matches!(
                (self.radio_group, other.radio_group),
                (RadioGroup::WifiChipset, RadioGroup::WifiChipset)
                    | (RadioGroup::BluetoothChipset, RadioGroup::BluetoothChipset)
                    | (RadioGroup::Usb, RadioGroup::Usb)
            )
    }
}
