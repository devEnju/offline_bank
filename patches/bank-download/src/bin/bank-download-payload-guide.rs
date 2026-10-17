//! What the download's guide in the release notes quotes, from the code that
//! shows it, one fact a line: the patch's name, then each result by name
//! with its two numbers as eight hexadecimal digits each. `New-Release.ps1`
//! writes the guide from it.

use bank_download_payload::download::{
    DONE, NO_BANK_ON_SERVER, PACKAGE, PRESENT, REFUSED, WRITTEN,
};

fn main() {
    let package = std::str::from_utf8(PACKAGE).expect("the name is ASCII");
    println!("package {package}");
    for (name, [first, second]) in [
        ("downloaded", [DONE, WRITTEN]),
        ("present", [DONE, PRESENT]),
        ("no-bank", [REFUSED, NO_BANK_ON_SERVER]),
    ] {
        println!("{name} {first:08X} {second:08X}");
    }
}
