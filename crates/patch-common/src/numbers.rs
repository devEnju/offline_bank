//! The screen every patch ends on after an error, and with a result: the
//! original's reason up to the end of its first part
//! (`patch_common::message`), and after it the page about the HOME Menu, the
//! original's line with the version and package on the second line and the
//! two numbers on the third. That page is complete or not changed at all.

use crate::message::{cut, first_line, CONTROL, LINE_FEED, NEW_PAGE};

/// Two numbers of eight digits with a space between them.
pub const LEN: usize = 17;

/// The version this package was built as. A release (`OFFLINE_BANK_RELEASE`,
/// which `cargo xtask release` sets, docs/building.md) has the
/// workspace's version from `Cargo.toml`; every other build is a draft and
/// has none, but names the commit it was built from (`OFFLINE_BANK_COMMIT`,
/// which `cargo xtask build` sets to the short hash), as "draft-a782e1b".
pub const VERSION: &[u8] = match option_env!("OFFLINE_BANK_RELEASE") {
    Some(_) => concat!("v", env!("CARGO_PKG_VERSION")).as_bytes(),
    None => &DRAFT_OF_COMMIT,
};
/// What a build that is no release says in place of a version.
pub const DRAFT: &[u8] = b"draft";
/// Hexadecimal digits of the commit a draft names.
pub const COMMIT_DIGITS: usize = 7;
const COMMIT: Option<&str> = option_env!("OFFLINE_BANK_COMMIT");
const DRAFT_LEN: usize = match COMMIT {
    Some(_) => DRAFT.len() + 1 + COMMIT_DIGITS,
    None => DRAFT.len(),
};
/// "draft", and "-" and the commit behind it when the build names one. A
/// commit that is not seven lowercase hexadecimal digits does not build.
const DRAFT_OF_COMMIT: [u8; DRAFT_LEN] = {
    let mut out = [0; DRAFT_LEN];
    let mut at = 0;
    while at < DRAFT.len() {
        out[at] = DRAFT[at];
        at += 1;
    }
    if let Some(commit) = COMMIT {
        let digits = commit.as_bytes();
        assert!(
            digits.len() == COMMIT_DIGITS,
            "OFFLINE_BANK_COMMIT is no short hash"
        );
        out[at] = b'-';
        let mut index = 0;
        while index < COMMIT_DIGITS {
            let digit = digits[index];
            assert!(
                digit.is_ascii_digit() || (digit >= b'a' && digit <= b'f'),
                "OFFLINE_BANK_COMMIT is no short hash"
            );
            out[at + 1 + index] = digit;
            index += 1;
        }
    }
    out
};

/// The line above the numbers: the version and, behind it, the package's
/// own suffix, as in "v1.2.3-test". For whoever reports the numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    units: [u16; Self::ROOM],
    len: usize,
}
impl Label {
    pub const ROOM: usize = 31;

    /// `None` for a text that is too long or holds anything but ASCII
    /// letters, digits, `.` and `-`: those are in the dialog's font in
    /// every language, as the numbers and the version line are.
    pub const fn new(version: &[u8], suffix: &[u8]) -> Option<Self> {
        let len = version.len() + suffix.len();
        if version.is_empty() || len > Self::ROOM {
            return None;
        }
        let mut units = [0; Self::ROOM];
        let mut at = 0;
        while at < len {
            let byte = if at < version.len() {
                version[at]
            } else {
                suffix[at - version.len()]
            };
            if !(byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-') {
                return None;
            }
            units[at] = byte as u16;
            at += 1;
        }
        Some(Self { units, len })
    }

    /// The label of this build for the package with `suffix`. A version
    /// that is no label does not build.
    pub const fn of(suffix: &[u8]) -> Self {
        match Self::new(VERSION, suffix) {
            Some(label) => label,
            None => panic!("the version with this suffix is not a label"),
        }
    }

    pub fn units(&self) -> &[u16] {
        &self.units[..self.len]
    }
}

/// The line of numbers: both as eight hexadecimal digits.
pub fn line(first: u32, second: u32) -> [u16; LEN] {
    let mut out = [0; LEN];
    let mut at = 0;
    for (index, value) in [first, second].into_iter().enumerate() {
        if index == 1 {
            out[at] = u16::from(b' ');
            at += 1;
        }
        for shift in (0..8).rev() {
            let digit = ((value >> (shift * 4)) & 15) as u8;
            out[at] = u16::from(if digit < 10 {
                b'0' + digit
            } else {
                b'A' + digit - 10
            });
            at += 1;
        }
    }
    out
}

/// The room for the page about the HOME Menu with its label and numbers.
/// The line has 11 to 42 units in the ten languages of the original.
const HOME_ROOM: usize = 128;

/// The page about the HOME Menu, complete: the original's line, `label` on
/// the second line and the numbers on the third.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomePage {
    units: [u16; HOME_ROOM],
    len: usize,
}
impl HomePage {
    pub fn units(&self) -> &[u16] {
        &self.units[..self.len]
    }
}

/// The page about the HOME Menu from `notice`, the original's line as it
/// was loaded. `None` unless it is one line and all of it, the label and
/// the numbers fit: the screen then shows the line alone, unchanged, and
/// never a part of what belongs under it.
pub fn home_page(notice: &[u16], label: &Label, first: u32, second: u32) -> Option<HomePage> {
    let label = label.units();
    if notice.is_empty() || first_line(notice) != notice.len() {
        return None;
    }
    let len = notice.len() + 1 + label.len() + 1 + LEN;
    if len > HOME_ROOM {
        return None;
    }
    let mut units = [0; HOME_ROOM];
    let mut at = 0;
    for part in [
        notice,
        &[LINE_FEED],
        label,
        &[LINE_FEED],
        &line(first, second),
    ] {
        units[at..at + part.len()].copy_from_slice(part);
        at += part.len();
    }
    Some(HomePage { units, len })
}

/// Replaces the message in `text` by `page` alone. `text` is the whole room
/// of the string, and the unit after the page stays free for its end mark.
/// Answers the new length, or `None` with the message untouched.
pub fn put_home(text: &mut [u16], page: &HomePage) -> Option<usize> {
    let page = page.units();
    if page.len() >= text.len() {
        return None;
    }
    text[..page.len()].copy_from_slice(page);
    Some(page.len())
}

/// The error screen in `text[..len]`, the original's reason as it was
/// loaded: its first part (`cut`), unchanged, and `home` as the page after
/// it. Answers the new length, or `None` with the message untouched if the
/// reason has no first part or the room is too small.
pub fn error_pages(text: &mut [u16], len: usize, home: &HomePage) -> Option<usize> {
    if len > text.len() {
        return None;
    }
    match cut(&text[..len]) {
        0 => None,
        end => append_page(text, end, home.units()),
    }
}

/// Adds `page` behind the message in `text[..len]` as one more page: the
/// page break, a line feed, as the original begins every later page of its
/// messages, and the page's text. Answers the new length, or `None` with
/// the message untouched if the room is too small. As in `put`, the unit
/// after the message stays free for its end mark.
fn append_page(text: &mut [u16], len: usize, page: &[u16]) -> Option<usize> {
    let lead = [CONTROL, 1, NEW_PAGE, LINE_FEED];
    let new_len = len.checked_add(lead.len())?.checked_add(page.len())?;
    if len > text.len() || new_len >= text.len() {
        return None;
    }
    text[len..len + lead.len()].copy_from_slice(&lead);
    text[len + lead.len()..new_len].copy_from_slice(page);
    Some(new_len)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::message::page_end;
    use std::{string::String, vec, vec::Vec};

    const PAGE: [u16; 3] = [CONTROL, 1, NEW_PAGE];

    /// A message with the given number of lines on each of its pages.
    fn message(pages: &[usize]) -> Vec<u16> {
        let mut out = Vec::new();
        for (page, &lines) in pages.iter().enumerate() {
            if page != 0 {
                out.extend(PAGE);
                // The original begins a later page with a line feed.
                out.push(LINE_FEED);
            }
            for line in 0..lines {
                if line != 0 {
                    out.push(LINE_FEED);
                }
                out.extend("text".encode_utf16());
            }
        }
        out
    }

    fn numbers() -> Vec<u16> {
        "00000003 7A000201".encode_utf16().collect()
    }
    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn both_numbers_are_eight_digits_with_a_space_between() {
        assert_eq!(line(3, 0x7a00_0201).to_vec(), numbers());
        assert_eq!(
            String::from_utf16(&line(0x13, 0xc880_4478)).unwrap(),
            "00000013 C8804478"
        );
        assert_eq!(
            String::from_utf16(&line(0, u32::MAX)).unwrap(),
            "00000000 FFFFFFFF"
        );
    }

    #[test]
    fn the_label_is_the_version_with_the_packages_suffix() {
        let text = |label: Label| String::from_utf16(label.units()).unwrap();
        assert_eq!(text(Label::new(b"v0.4.0", b"").unwrap()), "v0.4.0");
        assert_eq!(
            text(Label::new(b"v0.4.0", b"-test").unwrap()),
            "v0.4.0-test"
        );
        assert_eq!(text(Label::new(b"v10.20.30", b"").unwrap()), "v10.20.30");
        assert_eq!(
            text(Label::new(DRAFT, b"-migration").unwrap()),
            "draft-migration"
        );
        // This build's own: a draft, with the commit when the build names
        // one, or the version from Cargo.toml for a release.
        let own = match (option_env!("OFFLINE_BANK_RELEASE"), COMMIT) {
            (Some(_), _) => std::format!("v{}", env!("CARGO_PKG_VERSION")),
            (None, Some(commit)) => std::format!("draft-{commit}"),
            (None, None) => String::from("draft"),
        };
        assert_eq!(VERSION, own.as_bytes());
        assert_eq!(
            text(Label::of(b"-download")),
            std::format!("{own}-download")
        );
        // Nothing else is a label: only what every language's font has,
        // on one line, and never without a version.
        for (version, suffix) in [
            (&b"v0.4.0 "[..], &b""[..]),
            (b"v0.4.0", b"\n"),
            (b"v0.4.0", b"_test"),
            (b"v0.4.0\x10", b""),
            (b"\xc3\xa9", b""),
            (b"", b"-test"),
            (&[b'1'; Label::ROOM + 1], b""),
            (&[b'1'; Label::ROOM], b"-"),
        ] {
            assert_eq!(Label::new(version, suffix), None);
        }
        assert!(Label::new(&[b'1'; Label::ROOM], b"").is_some());
    }

    #[test]
    fn the_home_page_is_the_line_the_label_and_the_numbers() {
        let label = Label::new(b"v0.4.0", b"-test").unwrap();
        let page = home_page(&units("HOME"), &label, 3, 0x7a00_0201).unwrap();
        assert_eq!(
            String::from_utf16(page.units()).unwrap(),
            "HOME\nv0.4.0-test\n00000003 7A000201"
        );
        // A line that is no single line, or nothing, is not a page.
        assert_eq!(home_page(&units("A\nB"), &label, 3, 0), None);
        assert_eq!(home_page(&[], &label, 3, 0), None);
    }

    #[test]
    fn a_home_page_that_does_not_fit_is_none_and_never_shortened() {
        let label = Label::new(b"v0.4.0", b"-test").unwrap();
        let fits = HOME_ROOM - 2 - label.units().len() - LEN;
        assert!(home_page(&vec![0x41; fits], &label, 1, 2).is_some());
        assert_eq!(home_page(&vec![0x41; fits + 1], &label, 1, 2), None);
    }

    #[test]
    fn the_home_page_alone_replaces_the_line_where_it_fits() {
        let page = home_page(&units("HOME"), &Label::new(b"v1", b"").unwrap(), 1, 2).unwrap();
        let mut text = vec![0xffff; page.units().len() + 1];
        assert_eq!(put_home(&mut text, &page), Some(page.units().len()));
        assert_eq!(&text[..page.units().len()], page.units());
        // One unit stays free for the end mark.
        let mut text = vec![0xffff; page.units().len()];
        assert_eq!(put_home(&mut text, &page), None);
        assert!(text.iter().all(|&unit| unit == 0xffff));
    }

    /// The pages of a message, each without the line feed a later page
    /// begins with.
    fn pages(text: &[u16]) -> Vec<Vec<u16>> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(end) = page_end(rest, 1) {
            out.push(rest[..end].to_vec());
            assert_eq!(rest[end + PAGE.len()], LINE_FEED);
            rest = &rest[end + PAGE.len() + 1..];
        }
        out.push(rest.to_vec());
        out
    }

    #[test]
    fn a_page_is_added_behind_the_last_one_as_the_original_writes_pages() {
        let home: Vec<u16> = "home".encode_utf16().collect();
        // Messages of one, two and three pages, as the original has them.
        for shape in [&[1][..], &[3], &[2, 2], &[2, 3], &[1, 3, 2]] {
            let text = message(shape);
            let mut buffer = vec![0xffff; 128];
            buffer[..text.len()].copy_from_slice(&text);
            let len = append_page(&mut buffer, text.len(), &home).unwrap();
            // What was there is unchanged, and the new page is the last.
            assert_eq!(buffer[..text.len()], text);
            let all = pages(&buffer[..len]);
            assert_eq!(all.len(), shape.len() + 1);
            assert_eq!(all[..shape.len()], pages(&text));
            assert_eq!(all[shape.len()], home);
        }
    }

    #[test]
    fn the_error_screen_is_the_reason_as_it_is_and_then_the_home_page() {
        let label = Label::new(b"v1.2.3", b"-test").unwrap();
        let home = home_page(&message(&[1]), &label, 3, 0x7a00_0201).unwrap();
        // Bank's failure message, cut after its first page.
        let reason = message(&[2, 3]);
        let mut buffer = vec![0xffff; 128];
        buffer[..reason.len()].copy_from_slice(&reason);
        let len = error_pages(&mut buffer, reason.len(), &home).unwrap();
        let all: Vec<String> = pages(&buffer[..len])
            .iter()
            .map(|page| String::from_utf16(page).unwrap())
            .collect();
        assert_eq!(all, ["text\ntext", "text\nv1.2.3-test\n00000003 7A000201"]);
        // Without room for the page, or without a reason, nothing changes.
        let mut short = vec![0xffff; reason.len() + 4];
        short[..reason.len()].copy_from_slice(&reason);
        assert_eq!(error_pages(&mut short, reason.len(), &home), None);
        assert_eq!(short[..reason.len()], reason);
        assert_eq!(error_pages(&mut [0; 8], 0, &home), None);
    }

    #[test]
    fn a_page_without_room_is_not_added() {
        let text = message(&[2, 2]);
        let page = [0x41; 5];
        let needed = text.len() + 4 + page.len();
        let mut buffer = vec![0xffff; needed];
        buffer[..text.len()].copy_from_slice(&text);
        assert_eq!(append_page(&mut buffer, text.len(), &page), None);
        assert_eq!(buffer[..text.len()], text);
        assert!(buffer[text.len()..].iter().all(|&unit| unit == 0xffff));
        // One unit stays free for the end mark.
        buffer.push(0xffff);
        assert_eq!(append_page(&mut buffer, text.len(), &page), Some(needed));
        assert_eq!(append_page(&mut [0; 4], 5, &page), None);
    }
}
