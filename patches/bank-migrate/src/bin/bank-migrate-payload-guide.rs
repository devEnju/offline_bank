//! What the migration's guide in the release notes quotes, from the code that
//! shows it, one fact a line: each earlier way ("way", the last release that
//! stored the Bank so, and the second number of error 3 with which the
//! current release refuses such a Bank), the patch's name, and each result
//! by name with its numbers as eight hexadecimal digits. `New-Release.ps1`
//! writes its routes, error rows and guide from it; nothing else in the
//! project knows the earlier ways.

use bank_migrate_payload::{
    migrate::{numbers, Cause, Done, Step, Stopped, PACKAGE},
    source::Source,
};

fn main() {
    for source in Source::ALL {
        let tag = std::str::from_utf8(source.tag()).expect("a tag is ASCII");
        println!("way {tag} {:08X}", source.refused());
    }
    let package = std::str::from_utf8(PACKAGE).expect("the name is ASCII");
    println!("package {package}");
    let shown = |result| numbers::<()>(result, |_| 0);
    for (name, result) in [
        ("converted", Ok(Done::Converted)),
        ("already", Ok(Done::AlreadyNew)),
        ("none", Ok(Done::NoBank)),
        ("in-progress", Err(Stopped::InProgress)),
        ("unsettled", Err(Stopped::Unsettled)),
    ] {
        let [first, second] = shown(result);
        println!("{name} {first:08X} {second:08X}");
    }
    // A failed step: its first number, from the first step to the last.
    let failed = |step| shown(Err(Stopped::Failed(step, Cause::Missing)))[0];
    println!(
        "failed {:08X} {:08X}",
        failed(Step::ReadOld),
        failed(Step::WriteNew)
    );
}
