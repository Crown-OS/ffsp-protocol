#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::device::{Capabilities, DeviceClass, DeviceId};
use crate::link::{LinkRecord, LinkSecret};
use crate::medium::Medium;
use crate::util::hex;

fn link(seed: u8) -> LinkRecord {
    LinkRecord::new(
        [seed; 32],
        format!("device {seed}"),
        DeviceClass::Phone,
        Capabilities::WIFI_INFRA | Capabilities::BLUETOOTH_LE | Capabilities::AES_HARDWARE,
        LinkSecret::from_bytes([seed ^ 0xA5; 32]),
    )
}

fn advertisement_for(record: &LinkRecord, epoch: Epoch) -> Advertisement {
    Advertisement::new(
        record.class,
        ephemeral_id(&record.secret, epoch),
        epoch,
        record.capabilities,
    )
}

const NOW: Epoch = Epoch::new(1_000_000);

#[test]
fn an_advertisement_fits_the_legacy_budget_exactly() {
    assert_eq!(ADVERTISEMENT_LEN, 24);
    let encoded = advertisement_for(&link(1), NOW).to_bytes();
    assert_eq!(encoded.len(), ADVERTISEMENT_LEN);
}

#[test]
fn an_advertisement_round_trips() {
    let original = advertisement_for(&link(1), NOW);
    assert_eq!(
        Advertisement::parse(&original.to_bytes()).unwrap(),
        original
    );
}

#[test]
fn an_advertisement_of_the_wrong_length_is_rejected() {
    let encoded = advertisement_for(&link(1), NOW).to_bytes();
    for length in [0, 6, 23, 25] {
        let mut truncated = encoded.to_vec();
        truncated.resize(length, 0);
        assert_eq!(
            Advertisement::parse(&truncated),
            Err(DiscoveryError::AdvertisementLength {
                expected: 24,
                actual: length
            })
        );
    }
}

#[test]
fn an_advertisement_from_a_future_protocol_is_rejected() {
    let mut encoded = advertisement_for(&link(1), NOW).to_bytes();
    encoded[0] = 99;
    assert_eq!(
        Advertisement::parse(&encoded),
        Err(DiscoveryError::UnsupportedVersion(99))
    );
}

#[test]
fn an_advertisement_leaks_neither_the_device_id_nor_the_link_secret() {
    let record = link(7);
    let encoded = advertisement_for(&record, NOW).to_bytes();

    for secret_window in record.secret.as_bytes().windows(4) {
        assert!(
            !encoded.windows(4).any(|w| w == secret_window),
            "the beacon carries link secret material"
        );
    }
    for id_window in record.device.as_bytes().windows(4) {
        assert!(
            !encoded.windows(4).any(|w| w == id_window),
            "the beacon carries the device id"
        );
    }
}

#[test]
fn the_advertised_identifier_rotates_every_epoch() {
    let record = link(3);
    let now = ephemeral_id(&record.secret, NOW);
    let next = ephemeral_id(&record.secret, NOW.shift(1).unwrap());
    assert_ne!(now, next);
    assert_eq!(now, ephemeral_id(&record.secret, NOW));
}

#[test]
fn two_devices_never_advertise_the_same_identifier() {
    assert_ne!(
        ephemeral_id(&link(1).secret, NOW),
        ephemeral_id(&link(2).secret, NOW)
    );
}

#[test]
fn a_linked_peer_resolves_across_the_skew_window() {
    let records = vec![link(1), link(2)];
    let resolver = PresenceResolver::build(&records, NOW, PresenceResolver::DEFAULT_TOLERANCE);

    for delta in [-1, 0, 1] {
        let epoch = NOW.shift(delta).unwrap();
        let advertised = ephemeral_id(&records[1].secret, epoch);
        assert_eq!(
            resolver.resolve(&advertised),
            Some(records[1].device),
            "delta {delta}"
        );
    }
}

#[test]
fn a_peer_outside_the_skew_window_does_not_resolve_until_the_window_widens() {
    let records = vec![link(1)];
    let far = NOW.shift(3).unwrap();
    let advertised = ephemeral_id(&records[0].secret, far);

    let narrow = PresenceResolver::build(&records, NOW, PresenceResolver::DEFAULT_TOLERANCE);
    assert_eq!(narrow.resolve(&advertised), None);

    let wide = PresenceResolver::build(&records, NOW, PresenceResolver::SKEWED_TOLERANCE);
    assert_eq!(wide.resolve(&advertised), Some(records[0].device));
}

#[test]
fn an_unlinked_device_is_indistinguishable_from_noise() {
    let resolver = PresenceResolver::build(&[link(1)], NOW, 1);
    let stranger = ephemeral_id(&link(200).secret, NOW);
    assert_eq!(resolver.resolve(&stranger), None);
    assert_eq!(resolver.resolve(&EphemeralId::from_bytes([0; 16])), None);
}

#[test]
fn the_resolver_stays_constant_time_in_the_number_of_links() {
    let records: Vec<LinkRecord> = (0..50).map(link).collect();
    let resolver = PresenceResolver::build(&records, NOW, 1);
    assert_eq!(resolver.len(), 50 * 3);
    assert_eq!(
        resolver.resolve(&ephemeral_id(&records[49].secret, NOW)),
        Some(records[49].device)
    );
}

#[test]
fn a_beacon_reporting_an_epoch_outside_the_window_signals_clock_skew() {
    let resolver = PresenceResolver::build(&[link(1)], NOW, 1);
    assert!(resolver.covers_epoch_byte(NOW.low_byte()));
    assert!(resolver.covers_epoch_byte(NOW.shift(1).unwrap().low_byte()));
    assert!(!resolver.covers_epoch_byte(NOW.shift(40).unwrap().low_byte()));
}

#[test]
fn the_resolver_knows_when_its_epoch_has_passed() {
    let resolver = PresenceResolver::build(&[link(1)], NOW, 1);
    assert!(resolver.is_current_for(NOW));
    assert!(!resolver.is_current_for(NOW.shift(1).unwrap()));
}

#[test]
fn epochs_are_fifteen_minutes_and_do_not_underflow_at_zero() {
    assert_eq!(EPOCH_SECONDS, 900);
    assert_eq!(Epoch::from_unix_seconds(899), Epoch::new(0));
    assert_eq!(Epoch::from_unix_seconds(900), Epoch::new(1));
    assert_eq!(Epoch::new(0).shift(-1), None);
    assert_eq!(Epoch::new(0).window(2).count(), 3);
}

#[test]
fn the_mdns_record_carries_the_same_beacon_as_bluetooth() {
    let record = link(5);
    let advertisement = advertisement_for(&record, NOW);
    let txt = TxtRecord {
        advertisement,
        port: 53_317,
    };

    let pairs = txt.to_pairs();
    let parsed = TxtRecord::parse(|key| {
        pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    })
    .unwrap();

    assert_eq!(parsed, txt);
    assert_eq!(
        parsed.advertisement.ephemeral_id,
        advertisement.ephemeral_id
    );
    assert_eq!(SERVICE_TYPE, "_crowncrate._tcp");
}

#[test]
fn a_malformed_mdns_record_is_rejected() {
    assert_eq!(
        TxtRecord::parse(|_| None),
        Err(DiscoveryError::MissingTxtKey("d"))
    );
    assert_eq!(
        TxtRecord::parse(|key| if key == "d" { Some("zz") } else { Some("1") }),
        Err(DiscoveryError::MalformedTxtValue { key: "d" })
    );
    let valid = hex::encode(&advertisement_for(&link(1), NOW).to_bytes());
    assert_eq!(
        TxtRecord::parse(|key| if key == "d" {
            Some(&valid)
        } else {
            Some("not-a-port")
        }),
        Err(DiscoveryError::MalformedTxtValue { key: "p" })
    );
}

fn sighting_of(device: DeviceId, medium: Medium, at: u64) -> Sighting<'static> {
    Sighting {
        device,
        class: DeviceClass::Phone,
        capabilities: Capabilities::WIFI_INFRA,
        medium,
        endpoint: "endpoint".into(),
        observed_at_millis: at,
        signal_strength: None,
    }
}

#[test]
fn one_device_seen_over_three_mediums_is_one_peer() {
    let device = link(1).device;
    let mut tracker = PresenceTracker::default();

    assert_eq!(
        tracker.observe(sighting_of(device, Medium::BluetoothLe, 1_000)),
        PresenceChange::Appeared(device)
    );
    assert_eq!(
        tracker.observe(sighting_of(device, Medium::WifiInfra, 1_100)),
        PresenceChange::MediumAdded {
            device,
            medium: Medium::WifiInfra
        }
    );
    assert_eq!(
        tracker.observe(sighting_of(device, Medium::UsbAccessory, 1_200)),
        PresenceChange::MediumAdded {
            device,
            medium: Medium::UsbAccessory
        }
    );

    assert_eq!(tracker.len(), 1);
    let peer = tracker.get(&device).unwrap();
    assert_eq!(peer.mediums().count(), 3);
    assert_eq!(peer.last_seen_millis(), 1_200);
}

#[test]
fn re_observing_a_medium_refreshes_rather_than_duplicates() {
    let device = link(1).device;
    let mut tracker = PresenceTracker::default();
    tracker.observe(sighting_of(device, Medium::BluetoothLe, 1_000));
    assert_eq!(
        tracker.observe(sighting_of(device, Medium::BluetoothLe, 2_000)),
        PresenceChange::Refreshed(device)
    );
    assert_eq!(tracker.get(&device).unwrap().mediums().count(), 1);
    assert_eq!(tracker.get(&device).unwrap().last_seen_millis(), 2_000);
}

#[test]
fn usb_is_preferred_over_bluetooth_when_both_see_the_peer() {
    let device = link(1).device;
    let mut tracker = PresenceTracker::default();
    tracker.observe(sighting_of(device, Medium::BluetoothLe, 1_000));
    tracker.observe(sighting_of(device, Medium::UsbAccessory, 1_000));
    assert_eq!(
        tracker.get(&device).unwrap().fastest_medium(),
        Some(Medium::UsbAccessory)
    );
}

#[test]
fn a_peer_is_lost_only_when_every_medium_goes_quiet() {
    let device = link(1).device;
    let mut tracker = PresenceTracker::new(10_000);
    tracker.observe(sighting_of(device, Medium::BluetoothLe, 1_000));
    tracker.observe(sighting_of(device, Medium::WifiInfra, 9_000));

    assert!(tracker.expire(12_000).is_empty());
    assert_eq!(tracker.get(&device).unwrap().mediums().count(), 1);

    assert_eq!(tracker.expire(20_000), vec![device]);
    assert!(tracker.is_empty());
}

#[test]
fn withdrawing_the_last_medium_removes_the_peer() {
    let device = link(1).device;
    let mut tracker = PresenceTracker::default();
    tracker.observe(sighting_of(device, Medium::WifiInfra, 1_000));
    assert!(tracker.withdraw(&device, Medium::WifiInfra).is_some());
    assert!(tracker.is_empty());
    assert!(tracker.withdraw(&device, Medium::WifiInfra).is_none());
}
