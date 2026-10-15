#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::HashSet;

use super::*;
use crate::io::sealed_chunk_capacity;
use crate::transfer::CHUNK_LEN;
use crate::wire::{FrameHeader, LaneId};

fn registry_with(profiles: &[LaneProfile]) -> (LaneRegistry, Vec<LaneId>) {
    let mut registry = LaneRegistry::new();
    let lanes = profiles
        .iter()
        .map(|p| registry.allocate(*p).unwrap())
        .collect();
    (registry, lanes)
}

#[test]
fn every_medium_byte_round_trips() {
    for medium in Medium::ALL {
        assert_eq!(Medium::from_byte(medium.as_byte()).unwrap(), medium);
    }
}

#[test]
fn medium_bytes_are_stable() {
    assert_eq!(Medium::Loopback.as_byte(), 1);
    assert_eq!(Medium::WifiInfra.as_byte(), 2);
    assert_eq!(Medium::WifiDirect.as_byte(), 3);
    assert_eq!(Medium::UsbAccessory.as_byte(), 4);
    assert_eq!(Medium::BluetoothLe.as_byte(), 5);
    assert_eq!(Medium::BluetoothClassic.as_byte(), 6);
    assert!(Medium::from_byte(0).is_err());
    assert!(Medium::from_byte(7).is_err());
}

#[test]
fn a_default_registry_can_allocate() {
    let mut registry = LaneRegistry::default();
    assert!(registry.allocate(LaneProfile::LOOPBACK).is_ok());
}

#[test]
fn lane_ids_are_never_reused_even_after_removal() {
    let mut registry = LaneRegistry::new();
    let mut seen = HashSet::new();
    for _ in 0..1000 {
        let lane = registry.allocate(LaneProfile::LOOPBACK).unwrap();
        assert!(seen.insert(lane), "reissued {lane:?}");
        registry.remove(lane);
    }
    assert!(registry.is_empty());
}

#[test]
fn allocation_never_wraps_the_identifier_space() {
    let mut registry = LaneRegistry::new();
    for _ in 1..=u16::MAX {
        registry.allocate(LaneProfile::LOOPBACK).unwrap();
    }
    assert_eq!(
        registry.allocate(LaneProfile::LOOPBACK),
        Err(LaneRegistryError::LaneIdsExhausted)
    );
}

#[test]
fn the_control_lane_is_the_lowest_latency_one() {
    let (registry, lanes) = registry_with(&[
        LaneProfile::BLUETOOTH_LE,
        LaneProfile::WIFI_INFRA,
        LaneProfile::USB_ACCESSORY,
    ]);
    assert_eq!(registry.control_lane().unwrap(), lanes[2]);
}

#[test]
fn acknowledgements_leave_bluetooth_as_soon_as_anything_faster_exists() {
    let mut registry = LaneRegistry::new();
    let bluetooth = registry.allocate(LaneProfile::BLUETOOTH_LE).unwrap();
    assert_eq!(registry.control_lane().unwrap(), bluetooth);

    let wifi = registry.allocate(LaneProfile::WIFI_INFRA).unwrap();
    assert_eq!(registry.control_lane().unwrap(), wifi);
}

#[test]
fn a_parked_lane_carries_neither_control_nor_bulk() {
    let (mut registry, lanes) = registry_with(&[LaneProfile::WIFI_INFRA]);
    registry.set_parked(lanes[0], true);
    assert_eq!(
        registry.control_lane(),
        Err(LaneRegistryError::NoControlLane)
    );
    assert_eq!(registry.bulk_lanes().count(), 0);
    assert!(registry.schedulable_bulk_lanes().is_empty());
}

#[test]
fn the_two_bluetooth_lanes_collapse_to_the_one_that_carries_bulk() {
    let (registry, lanes) =
        registry_with(&[LaneProfile::BLUETOOTH_LE, LaneProfile::BLUETOOTH_CLASSIC]);
    // They share a chipset, so only one of them can be scheduled, and low energy cannot be it.
    assert_eq!(registry.schedulable_bulk_lanes(), vec![lanes[1]]);
    assert!(registry.control_lane().is_ok());
}

#[test]
fn usb_and_wifi_are_scheduled_together_because_they_do_not_contend() {
    let (registry, lanes) = registry_with(&[LaneProfile::WIFI_INFRA, LaneProfile::USB_ACCESSORY]);
    let schedulable = registry.schedulable_bulk_lanes();
    assert_eq!(schedulable.len(), 2);
    assert!(schedulable.contains(&lanes[0]));
    assert!(schedulable.contains(&lanes[1]));
}

#[test]
fn two_wifi_lanes_collapse_to_the_faster_one() {
    let (registry, lanes) = registry_with(&[LaneProfile::WIFI_DIRECT, LaneProfile::WIFI_INFRA]);
    assert!(LaneProfile::WIFI_INFRA.contends_with(&LaneProfile::WIFI_DIRECT));
    assert_eq!(registry.schedulable_bulk_lanes(), vec![lanes[1]]);
}

#[test]
fn a_measurement_can_overturn_the_bandwidth_prior() {
    let (mut registry, lanes) = registry_with(&[LaneProfile::WIFI_DIRECT, LaneProfile::WIFI_INFRA]);
    registry.observe(lanes[0], 3_000, 900_000);
    registry.observe(lanes[1], 9_000, 20_000);
    assert_eq!(registry.schedulable_bulk_lanes(), vec![lanes[0]]);
    assert_eq!(registry.control_lane().unwrap(), lanes[0]);
}

#[test]
fn loopback_lanes_do_not_contend_so_tests_can_stripe_over_many() {
    let (registry, lanes) = registry_with(&[
        LaneProfile::LOOPBACK,
        LaneProfile::LOOPBACK,
        LaneProfile::LOOPBACK,
    ]);
    assert_eq!(registry.schedulable_bulk_lanes(), lanes);
}

#[test]
fn low_energy_signals_but_never_carries() {
    assert!(!LaneProfile::BLUETOOTH_LE.carries_bulk());
    assert!(LaneProfile::BLUETOOTH_LE.carries_control());
}

/// Slow is not the same as useless, and a benchmark that excluded Bluetooth could never say what
/// Bluetooth contributes. The scheduler sizes its share from what it measures.
#[test]
fn classic_bluetooth_carries_bulk_so_its_contribution_can_be_measured() {
    assert!(LaneProfile::BLUETOOTH_CLASSIC.carries_bulk());
    assert!(LaneProfile::BLUETOOTH_CLASSIC.carries_control());
}

#[test]
fn the_usb_transfer_limit_matches_the_accessory_driver_cap() {
    assert_eq!(
        LaneProfile::USB_ACCESSORY.max_transfer_len,
        AOA_MAX_TRANSFER
    );
    assert_eq!(AOA_MAX_TRANSFER, 16_384);
}

/// A bulk lane whose frame limit sat below one sealed chunk could never carry one, however
/// small its individual transfers are.
#[test]
fn every_bulk_lane_admits_a_whole_sealed_chunk() {
    let sealed = FrameHeader::LEN + sealed_chunk_capacity(CHUNK_LEN);
    for medium in Medium::ALL {
        let profile = LaneProfile::of(medium);
        if profile.carries_bulk() {
            assert!(
                profile.max_frame_len >= sealed,
                "{medium} cannot carry a chunk"
            );
        }
    }
}

#[test]
fn every_medium_has_a_profile_naming_itself() {
    for medium in Medium::ALL {
        assert_eq!(LaneProfile::of(medium).medium, medium);
    }
}

#[test]
fn the_accessory_identity_strings_are_the_ones_android_filters_on() {
    // Kept in lockstep with crowncrate-android's res/xml/usb_accessory_filter.xml.
    assert_eq!(AOA_MANUFACTURER, "CrownOS");
    assert_eq!(AOA_MODEL, "CrownCrate");
    assert_eq!(AOA_VERSION, "1.0");
    for field in [
        AOA_MANUFACTURER,
        AOA_MODEL,
        AOA_VERSION,
        AOA_DESCRIPTION,
        AOA_URI,
    ] {
        assert!(!field.is_empty());
        assert!(
            field.is_ascii(),
            "the accessory protocol carries these as ASCII"
        );
    }
}

/// A released identifier carries a used nonce counter with it. Handing it out again would pair a
/// counter restarting at zero with a key that has already sealed under those nonces.
#[test]
fn a_released_identifier_is_never_handed_out_again() {
    let mut registry = LaneRegistry::new();
    let first = registry.allocate(LaneProfile::WIFI_INFRA).unwrap();
    registry.remove(first);
    let second = registry.allocate(LaneProfile::WIFI_INFRA).unwrap();
    assert_ne!(first, second);
    assert!(matches!(
        registry.adopt(first, LaneProfile::WIFI_INFRA),
        Err(LaneRegistryError::LaneAlreadyIssued { .. })
    ));
}

#[test]
fn adopting_and_allocating_never_collide_whatever_the_order() {
    let mut registry = LaneRegistry::new();
    registry
        .adopt(LaneId::new(4), LaneProfile::WIFI_INFRA)
        .unwrap();
    registry
        .adopt(LaneId::new(2), LaneProfile::USB_ACCESSORY)
        .unwrap();
    let mine = registry.allocate(LaneProfile::BLUETOOTH_CLASSIC).unwrap();
    assert!(mine != LaneId::new(4) && mine != LaneId::new(2));
    assert!(matches!(
        registry.adopt(mine, LaneProfile::WIFI_INFRA),
        Err(LaneRegistryError::LaneAlreadyIssued { .. })
    ));
}
