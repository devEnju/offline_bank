//! The two things the patch does with Bank's files, and the numbers it
//! shows. Both go through `bank-common`, as every write of the offline
//! patch does, so what is written here is what the offline patch reads.

use bank_common::bank_files::{self, BankFiles, Files};
use offline_core::{native_blob, Storage};

/// First number of a result that is no error.
pub const DONE: u32 = 0xd0ad;
/// Second number: the Bank was downloaded and written.
pub const WRITTEN: u32 = 1;
/// Second number: a Bank was on the card already, and nothing was done.
pub const PRESENT: u32 = 2;
/// First number when the patch stopped before doing anything.
pub const REFUSED: u32 = 0xbad;
/// Second number: the server holds no Bank, and the patch creates none.
pub const NO_BANK_ON_SERVER: u32 = 1;
/// The patch's name: behind "Ver. 1.5" on the start screen, and behind the
/// version on the result screen.
pub const PACKAGE: &[u8] = b"-download";

type Outcome<T, F> = Result<T, bank_files::Error<<<F as Files>::Storage as Storage>::Error>>;

/// Whether the card holds a Bank of the offline patch. A Bank file whose
/// creation was cut before a Bank became current in it counts as none; the
/// next download finishes it (`BankFiles::open`). Nothing is written.
pub fn present<F: Files>(files: F) -> Outcome<bool, F> {
    Ok(BankFiles::new(files).open()?.is_some())
}

/// Writes the native Bank body in `staging` as a new Bank: the main file
/// and the transport box first, the Bank file last. `false`, with nothing
/// written, if a Bank is on the card after all: one is never replaced.
///
/// What only the server filled, and the places of the patch's Miles state,
/// are written as a new Bank has them (`native_blob::clear_unused`): nothing
/// of the server's is kept that the Bank does not need, no remainder, never
/// redeemed.
pub fn write<F: Files>(files: F, staging: &mut [u8]) -> Outcome<bool, F> {
    let mut files = BankFiles::new(files);
    if files.open()?.is_some() {
        return Ok(false);
    }
    native_blob::clear_unused(staging);
    files.initialize(staging)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use bank_common::testing::*;
    use std::{format, vec::Vec};

    /// `body` as a download stores it: without the unused places.
    fn stored(mut body: Vec<u8>) -> Vec<u8> {
        for range in native_blob::UNUSED {
            body[range].fill(0);
        }
        body
    }

    #[test]
    fn an_empty_card_gets_the_bank_as_it_was_loaded_and_a_second_run_leaves_it() {
        let disk = Shared::default();
        assert_eq!(present(disk.clone()), Ok(false));
        // Boxes, Pokédex and a transport box with Pokémon in it.
        let loaded = body(0x31, 0x44, 7);
        assert_eq!(write(disk.clone(), &mut loaded.clone()), Ok(true));
        let (bytes, read) = load(&mut opened(&disk.reboot()));
        assert!(bytes == stored(loaded.clone()));
        assert!(!read.transport_missing);

        // A Bank is there now: found, and never written over.
        assert_eq!(present(disk.reboot()), Ok(true));
        let other = body(0x52, 0x66, 0);
        assert_eq!(write(disk.reboot(), &mut other.clone()), Ok(false));
        assert!(load(&mut opened(&disk.reboot())).0 == stored(loaded));
    }

    #[test]
    fn damaged_or_unreadable_local_data_is_never_treated_as_an_empty_card() {
        for unreadable in [false, true] {
            let disk = Shared::default();
            assert_eq!(write(disk.clone(), &mut body(0x31, 0x44, 7)), Ok(true));
            {
                let mut state = disk.0.borrow_mut();
                let bank = bank_files::FileName::Bank as u8;
                if unreadable {
                    state.unreadable.insert((bank, 0));
                    state.unreadable.insert((bank, 1));
                } else {
                    let bytes = state.files.get_mut(&bank).unwrap();
                    bytes[0] ^= 1;
                    bytes[bank_files::RECORD as usize] ^= 1;
                }
            }
            let before = disk.0.borrow().files.clone();
            assert!(present(disk.clone()).is_err());
            assert!(write(disk.clone(), &mut body(0x52, 0x66, 0)).is_err());
            assert_eq!(disk.0.borrow().files, before);
        }
    }

    #[test]
    fn an_inspection_error_never_authorizes_creating_files() {
        struct Inaccessible;
        impl bank_files::Files for Inaccessible {
            type Storage = Handle;
            fn open(&mut self, _: bank_files::FileName) -> Result<Option<Handle>, ()> {
                Err(()) // Includes native wrong-size, permission and I/O failures.
            }
            fn create(&mut self, _: bank_files::FileName) -> Result<Handle, ()> {
                panic!("a failed inspection must not reach creation")
            }
        }
        assert!(present(Inaccessible).is_err());
        assert!(write(Inaccessible, &mut body(0x52, 0x66, 0)).is_err());
    }

    #[test]
    fn a_downloaded_bank_keeps_nothing_of_the_servers_but_the_bank() {
        // The server's pass count, here one that reads as a date, and
        // something in every other unused place.
        let mut loaded = body(0x31, 0x44, 0);
        for range in native_blob::UNUSED {
            loaded[range].fill(0x5a);
        }
        let at = native_blob::REDEEMED_OFFSET;
        loaded[at..at + 4].copy_from_slice(&[0xea, 0x07, 10, 6]);
        let disk = Shared::default();
        assert_eq!(write(disk.clone(), &mut loaded.clone()), Ok(true));
        let stored = load(&mut opened(&disk.reboot())).0;
        let view = native_blob::NativeBlobView::parse_layout(&stored).unwrap();
        assert_eq!(view.rewards(), offline_core::rewards::Record::NONE);
        for range in native_blob::UNUSED {
            assert!(
                stored[range.clone()].iter().all(|&byte| byte == 0),
                "{range:x?}"
            );
        }
        // Nothing else differs from what was loaded.
        assert!(stored == self::stored(loaded));
    }

    #[test]
    fn a_write_cut_at_any_point_counts_as_no_bank_and_the_next_one_finishes_it() {
        for console in [false, true] {
            let loaded = body(0x31, 0x44, 3);
            let empty = Shared::default().like(console);
            let counting = empty.cut_after(usize::MAX);
            assert_eq!(write(counting.clone(), &mut loaded.clone()), Ok(true));
            let total = counting.0.borrow().operations;
            assert!(total > 20);
            for cut in 0..total {
                let context = format!("console {console}, cut {cut}");
                let disk = empty.cut_after(cut);
                assert!(
                    write(disk.clone(), &mut loaded.clone()).is_err(),
                    "{context}"
                );
                let after = disk.reboot();
                match present(after.clone()) {
                    // The Bank became current with the last write that
                    // counts: it is complete.
                    Ok(true) => assert!(
                        load(&mut opened(&after)).0 == stored(loaded.clone()),
                        "{context}"
                    ),
                    // Not yet: the next download writes it whole.
                    Ok(false) => {
                        assert_eq!(
                            write(after.clone(), &mut loaded.clone()),
                            Ok(true),
                            "{context}"
                        );
                        assert!(
                            load(&mut opened(&after.reboot())).0 == stored(loaded.clone()),
                            "{context}"
                        );
                    }
                    Err(error) => panic!("{context}: {error:?}"),
                }
            }
        }
    }
}
