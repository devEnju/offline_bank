//! The journal of this version of the stored files, as every release of it
//! wrote it. A later release of the same version reads all of it; one that
//! does not is a new version (docs/internals.md, "Files", its rule for new
//! features).

use offline_core::{Metadata, Phase, METADATA_SIZE, SCHEMA_VERSION};

/// Journal records as a release wrote them, taken from its sources before it
/// was released: at rest after creating a Bank, with a delivery from
/// Transporter waiting, and with a save in progress (a cartridge of title
/// `0004000000055D00`, trainer "ASH" with ID 12345, from v0.7.0 on the
/// whole ID of secret ID 8 and Trainer ID 12345, fingerprints of all 1 and
/// all 2). Every release of this version adds its own.
const RELEASES: [(&str, [&str; 3]); 3] = [(
    "v0.5.0",
    [
        "424b4a010100000000000000606ec39112c569f89adde7d80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041f1df97",
        "424b4a010200000002000000606ec39112c569f89adde7d800000000000000000000000096be08cd000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d30f5d5e",
        "424b4a010300000013180002606ec39112c569f89adde7d8d5bea0d0aa861fd28f435e8196be08cd41005300480000000000000000000000000000000000000039300000389bdfc501010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202ef3896b9",
    ],
), (
    "v0.6.0",
    [
        "424b4a0101000000000000009525807c150b78a19adde7d80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000095ff5d90",
        "424b4a0102000000020000009525807c150b78a19adde7d8000000000000000000000000a9933863000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b64680a1",
        "424b4a0103000000131800019525807c150b78a19adde7d89525807c150b78a1a9933863a993386341005300480000000000000000000000000000000000000039300000389bdfc501010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202a87f7a97",
    ],
), (
    "v0.7.0",
    [
        "424b4a0101000000000000009525807c150b78a19adde7d80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000095ff5d90",
        "424b4a0102000000020000009525807c150b78a19adde7d8000000000000000000000000a9933863000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b64680a1",
        "424b4a0103000000131800019525807c150b78a19adde7d89525807c150b78a1a9933863a993386341005300480000000000000000000000000000000000000039300800b650b9e601010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202f0bec264",
    ],
)];

/// The save in progress of a release's `RELEASES`, as the release wrote it
/// once Bank left it to Transporter's question (bit 2 of byte 8), and then
/// once it was unlocked (bit 3). Every release of this version from v0.7.0
/// on adds its own.
const QUESTION: [(&str, [&str; 2]); 1] = [(
    "v0.7.0",
    [
        "424b4a0104000000171800019525807c150b78a19adde7d89525807c150b78a1a9933863a993386341005300480000000000000000000000000000000000000039300800b650b9e6010101010101010101010101010101010101010101010101010101010101010102020202020202020202020202020202020202020202020202020202020202020d0a7602",
        "424b4a01050000001f1800019525807c150b78a19adde7d89525807c150b78a1a9933863a993386341005300480000000000000000000000000000000000000039300800b650b9e601010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202c4df9d3b",
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
        // Bits 2 and 3 of byte 8 were unused before Transporter's question
        // and the unlock: no earlier record leaves a save in progress to it.
        for read in [at_rest, delivery, in_progress] {
            assert!(!read.ask && !read.unlocked, "{release}");
        }
    }
}

#[test]
fn every_save_left_to_transporters_question_a_release_wrote_is_read_as_it_was_meant() {
    for (release, records) in QUESTION {
        for record in records {
            let read = Metadata::decode(&bytes(record)).expect(release);
            assert_eq!(read.encode().unwrap(), bytes(record), "{release}");
        }
        let (_, [_, _, in_progress]) = RELEASES
            .iter()
            .find(|(written, _)| *written == release)
            .expect(release);
        let in_progress = Metadata::decode(&bytes(in_progress)).expect(release);
        let [asked, unlocked] =
            records.map(|record| Metadata::decode(&bytes(record)).expect(release));
        // Left to the question: Transporter asks, but only while Bank finds
        // the save's own game.
        assert!(asked.ask && !asked.unlocked && asked.asks(), "{release}");
        // Unlocked: Transporter asks whatever game Bank finds.
        assert!(
            unlocked.ask && unlocked.unlocked && unlocked.asks(),
            "{release}"
        );
        assert!(unlocked.sequence > asked.sequence, "{release}");
        assert!(asked.sequence > in_progress.sequence, "{release}");
        // The save in progress stays as it was.
        for read in [asked, unlocked] {
            assert_eq!(read.phase, in_progress.phase, "{release}");
            assert_eq!(read.delivery, in_progress.delivery, "{release}");
        }
    }
}
