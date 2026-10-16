//! The names of the offline Bank's files in Bank's extdata (`0x00000C9B`),
//! which Bank and Transporter both reach. Each file exists in two copies:
//! the first name is copy A, `.alt` names copy B, the alternate of the two.
//! A name is at most 16 characters after its slash, the console's limit.

/// The two journal records.
pub const JOURNAL: [&str; 2] = ["/journal.bin", "/journal.alt.bin"];
/// The two copies of the Bank file: the boxes.
pub const BANK: [&str; 2] = ["/bank.bin", "/bank.alt.bin"];
/// The two copies of the main file.
pub const MAIN: [&str; 2] = ["/main.bin", "/main.alt.bin"];
/// The two copies of the transport box.
pub const MOVER: [&str; 2] = ["/mover.bin", "/mover.alt.bin"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_fits_the_console_limit() {
        for name in JOURNAL.into_iter().chain(BANK).chain(MAIN).chain(MOVER) {
            assert!(name.starts_with('/') && name.is_ascii());
            assert!(name.len() - 1 <= 16, "{name}");
        }
    }
}
