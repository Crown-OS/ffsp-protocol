/// The three strings a USB host sends to put an Android device into accessory mode.
///
/// These must byte-match `res/xml/usb_accessory_filter.xml` in `crowncrate-android`. A mismatch
/// produces no intent and no error on either side: the device re-enumerates, no app is launched,
/// and nothing anywhere says why. Both platforms read these constants so the two cannot drift, and
/// a test on the Linux side parses the Android XML to assert they still agree.
pub const AOA_MANUFACTURER: &str = "CrownOS";
pub const AOA_MODEL: &str = "CrownCrate";
pub const AOA_VERSION: &str = "1.0";
pub const AOA_DESCRIPTION: &str = "CrownCrate file transfer";
pub const AOA_URI: &str = "https://crownos.io/crowncrate";
