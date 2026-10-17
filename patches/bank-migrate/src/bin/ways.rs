//! The earlier ways the migration converts, one line each: the last release
//! that stored the Bank so, and the second number of error 3 with which the
//! current release refuses such a Bank, as eight hexadecimal digits.
//! `New-Release.ps1` writes its routes and error rows from it; nothing else
//! in the project knows the earlier ways.

use bank_migrate_payload::source::Source;

fn main() {
    for source in Source::ALL {
        let tag = std::str::from_utf8(source.tag()).expect("a tag is ASCII");
        println!("{tag} {:08X}", source.refused());
    }
}
