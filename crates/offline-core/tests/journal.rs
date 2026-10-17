//! The journal of this version of the stored files, as every release of it
//! wrote it. A later release of the same version reads all of it; one that
//! does not is a new version (docs/internals.md, "Rule for new features").

use offline_core::{Metadata, Phase, METADATA_SIZE, SCHEMA_VERSION};

/// Journal records as a release wrote them, taken from its tag: at rest after
/// creating a Bank, with a delivery from Transporter waiting, and with a save
/// in progress (a cartridge of title `0004000000055D00`, trainer "ASH" with
/// ID 12345, fingerprints of all 1 and all 2). Every release of this version
/// adds its own.
const RELEASES: [(&str, [&str; 3]); 1] = [(
    "v0.5.0",
    [
        "424b4a010100000000000000606ec39112c569f89adde7d80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041f1df97",
        "424b4a010200000002000000606ec39112c569f89adde7d800000000000000000000000096be08cd000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d30f5d5e",
        "424b4a010300000013180002606ec39112c569f89adde7d8d5bea0d0aa861fd28f435e8196be08cd41005300480000000000000000000000000000000000000039300000389bdfc501010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202ef3896b9",
    ],
)];

fn bytes(hex: &str) -> [u8; METADATA_SIZE] {
    let mut out = [0; METADATA_SIZE];
    assert_eq!(hex.len(), METADATA_SIZE * 2);
    for (at, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).unwrap();
    }
    out
}

#[test]
fn the_journal_version_is_the_major_number_of_this_crate() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        format!("{SCHEMA_VERSION}.0.0"),
        "offline-core's version is that of the stored files, a whole number"
    );
}

#[test]
fn every_record_a_release_of_this_version_wrote_is_read_as_it_was_meant() {
    for (release, records) in RELEASES {
        // Written again, each is the same bytes: nothing in it is lost.
        for record in records {
            let read = Metadata::decode(&bytes(record)).expect(release);
            assert_eq!(read.encode().unwrap(), bytes(record), "{release}");
        }
        let [at_rest, delivery, in_progress] = records;
        let at_rest = Metadata::decode(&bytes(at_rest)).expect(release);
        assert!(matches!(at_rest.phase, Phase::Clean(_)), "{release}");
        assert_eq!(at_rest.delivery, None, "{release}");

        let delivery = Metadata::decode(&bytes(delivery)).expect(release);
        assert!(matches!(delivery.phase, Phase::Clean(_)), "{release}");
        assert!(delivery.delivery.is_some(), "{release}");
        assert!(delivery.sequence > at_rest.sequence, "{release}");

        let in_progress = Metadata::decode(&bytes(in_progress)).expect(release);
        let Phase::Prepared(pending) = in_progress.phase else {
            panic!("{release}: no save in progress");
        };
        assert_eq!(pending.game.title_id, 0x0004_0000_0005_5d00, "{release}");
        assert_eq!(pending.before_fingerprint, [1; 32], "{release}");
        assert_eq!(pending.after_fingerprint, [2; 32], "{release}");
        assert_eq!(pending.before, delivery.phase.current(), "{release}");
    }
}
