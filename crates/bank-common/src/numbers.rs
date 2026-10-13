//! What a patch does to a message of the original for its last screen: the
//! cut after its first part, the line of two numbers with the line above it
//! that says which version and package shows them, and the page that
//! follows.
//!
//! A message is the original's own text in UTF-16 units. A line feed ends a
//! line; a control code is the unit `0x10`, the number of units that follow,
//! then the code and its arguments; the code `0xBE01` ends a page (wait for a
//! button, then clear). The dialog holds three lines, and the numbers are
//! always its third: only a first part of one or two lines gets them. The
//! label is the second line, where the first part leaves it free.

/// A control code in the original's texts.
const CONTROL: u16 = 0x10;
/// The control code between two pages.
const NEW_PAGE: u16 = 0xbe01;
const LINE_FEED: u16 = 0x0a;
/// Two numbers of eight digits with a space between them.
pub const LEN: usize = 17;
/// The dialog's last line.
const LINE: usize = 3;

/// The version this package was built as: the release script names it
/// (`OFFLINE_BANK_VERSION`, docs/building.md). A package built without one
/// is nobody's release.
pub const VERSION: &[u8] = match option_env!("OFFLINE_BANK_VERSION") {
    Some(version) => version.as_bytes(),
    None => b"dev",
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
            None => panic!("OFFLINE_BANK_VERSION with this suffix is not a label"),
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

/// Where the first page of `text` ends, and where its first line does.
fn first_part(text: &[u16]) -> (Option<usize>, Option<usize>) {
    let mut line = None;
    let mut at = 0;
    while at < text.len() {
        if text[at] != CONTROL {
            if text[at] == LINE_FEED && line.is_none() {
                line = Some(at);
            }
            at += 1;
            continue;
        }
        if at + 2 >= text.len() {
            break;
        }
        if text[at + 2] == NEW_PAGE {
            return (Some(at), line);
        }
        at += 2 + usize::from(text[at + 1]);
    }
    (None, line)
}

/// Where the first part of the message `text` ends: its first page or, in
/// a message of one page, its first line. Nothing is written; a message
/// that ends there is the original's own text up to that point.
pub fn cut(text: &[u16]) -> usize {
    let (page, feed) = first_part(text);
    page.or(feed).unwrap_or(text.len())
}

/// Ends the message in `text[..len]` after its first part (`cut`) and puts
/// the numbers on the third line there, with `label` on the second if the
/// first part is one line. `text` is the whole room of the string, and the
/// unit after the message stays free for its end mark. Without room for
/// the label the numbers go alone. Answers the new length, or `None` with
/// the message untouched: its first page has three lines already, or the
/// room is too small for the numbers.
pub fn put(text: &mut [u16], len: usize, label: &[u16], first: u32, second: u32) -> Option<usize> {
    if len > text.len() {
        return None;
    }
    let end = cut(&text[..len]);
    let lines = 1 + text[..end]
        .iter()
        .filter(|&&unit| unit == LINE_FEED)
        .count();
    if lines >= LINE {
        return None;
    }
    let feeds = LINE - lines;
    // Only a free line between takes the label.
    let label = if feeds == 2 { label } else { &[] };
    let new_len = end + feeds + label.len() + LEN;
    if new_len >= text.len() {
        return if label.is_empty() {
            None
        } else {
            put(text, len, &[], first, second)
        };
    }
    let numbers = new_len - LEN;
    text[end] = LINE_FEED;
    text[end + 1..end + 1 + label.len()].copy_from_slice(label);
    text[numbers - 1] = LINE_FEED;
    text[numbers..new_len].copy_from_slice(&line(first, second));
    Some(new_len)
}

/// Adds `page` behind the message in `text[..len]` as one more page: the
/// page break, a line feed, as the original begins every later page of its
/// messages, and the page's text. Answers the new length, or `None` with
/// the message untouched if the room is too small. As in `put`, the unit
/// after the message stays free for its end mark.
pub fn append_page(text: &mut [u16], len: usize, page: &[u16]) -> Option<usize> {
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
    fn put_in(text: &[u16], room: usize) -> Option<Vec<u16>> {
        let mut buffer = vec![0xffff; room];
        buffer[..text.len()].copy_from_slice(text);
        let len = put(&mut buffer, text.len(), &[], 3, 0x7a00_0201)?;
        assert!(len < room);
        // Nothing behind the new end was needed for it.
        Some(buffer[..len].to_vec())
    }
    fn numbers() -> Vec<u16> {
        "00000003 7A000201".encode_utf16().collect()
    }
    /// The lines of the first page, as text.
    fn first_page(text: &[u16]) -> Vec<String> {
        let end = first_part(text).0.unwrap_or(text.len());
        String::from_utf16(&text[..end])
            .unwrap()
            .split('\n')
            .map(String::from)
            .collect()
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
    fn the_message_ends_with_the_numbers_on_the_third_line() {
        // One line: an empty line between, as under the HOME line.
        let out = put_in(&message(&[1]), 64).unwrap();
        assert_eq!(first_page(&out), ["text", "", "00000003 7A000201"]);
        // Bank's failure message: its first page has one or two lines, and
        // what follows that page is dropped.
        for first in [1, 2] {
            let out = put_in(&message(&[first, 2]), 96).unwrap();
            assert_eq!(first_part(&out).0, None);
            assert_eq!(out[out.len() - LEN..], numbers());
            assert_eq!(first_page(&out).len(), 3);
        }
        let out = put_in(&message(&[2, 3]), 96).unwrap();
        assert_eq!(first_page(&out), ["text", "text", "00000003 7A000201"]);
        // One language has it on one page, its first sentence on one line.
        let out = put_in(&message(&[2]), 64).unwrap();
        assert_eq!(first_page(&out), ["text", "", "00000003 7A000201"]);
    }

    #[test]
    fn the_label_is_the_version_with_the_packages_suffix() {
        let text = |label: Label| String::from_utf16(label.units()).unwrap();
        assert_eq!(text(Label::new(b"v0.4.0", b"").unwrap()), "v0.4.0");
        assert_eq!(
            text(Label::new(b"v0.4.0", b"-test").unwrap()),
            "v0.4.0-test"
        );
        assert_eq!(
            text(Label::new(b"v10.20.30-draft", b"").unwrap()),
            "v10.20.30-draft"
        );
        assert_eq!(
            text(Label::new(b"dev", b"-migration").unwrap()),
            "dev-migration"
        );
        // This build's own: the version it was given, or "dev".
        assert_eq!(
            text(Label::of(b"-download")),
            std::format!("{}-download", String::from_utf8(VERSION.to_vec()).unwrap())
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
    fn the_label_is_the_second_line_where_that_line_is_free() {
        let label: Vec<u16> = "v0.4.0-test".encode_utf16().collect();
        let with = |text: &[u16], room: usize| {
            let mut buffer = vec![0xffff; room];
            buffer[..text.len()].copy_from_slice(text);
            let len = put(&mut buffer, text.len(), &label, 3, 0x7a00_0201)?;
            assert!(len < room);
            Some(buffer[..len].to_vec())
        };
        // The line about the HOME Menu is one line in every language.
        let out = with(&message(&[1]), 64).unwrap();
        assert_eq!(
            first_page(&out),
            ["text", "v0.4.0-test", "00000003 7A000201"]
        );
        // A first part of two lines has no line for it: the numbers alone,
        // where they always are.
        let out = with(&message(&[2, 3]), 96).unwrap();
        assert_eq!(first_page(&out), ["text", "text", "00000003 7A000201"]);
        // Without room for both, the numbers alone; without room for
        // them, nothing.
        let text = message(&[1]);
        let plain = put_in(&text, 64).unwrap();
        let full = plain.len() + label.len();
        assert_eq!(with(&text, full + 1).unwrap().len(), full);
        assert_eq!(with(&text, full).unwrap(), plain);
        assert_eq!(with(&text, plain.len() + 1).unwrap(), plain);
        assert_eq!(with(&text, plain.len()), None);
    }

    #[test]
    fn a_first_page_of_three_lines_is_left_alone() {
        assert_eq!(put_in(&message(&[3, 3]), 128), None);
        assert_eq!(put_in(&message(&[4, 1]), 128), None);
    }

    #[test]
    fn without_room_nothing_is_written() {
        let text = message(&[2, 2]);
        let needed = put_in(&text, 128).unwrap().len();
        // One unit stays free for the end mark.
        assert!(put_in(&text, needed + 1).is_some());
        let mut buffer = vec![0xffff; needed];
        buffer[..text.len()].copy_from_slice(&text);
        assert_eq!(put(&mut buffer, text.len(), &[], 1, 2), None);
        assert_eq!(buffer[..text.len()], text);
        assert_eq!(put(&mut [0; 4], 5, &[], 1, 2), None);
    }

    /// The pages of a message, each without the line feed a later page
    /// begins with.
    fn pages(text: &[u16]) -> Vec<Vec<u16>> {
        let mut out = Vec::new();
        let mut rest = text;
        while let (Some(end), _) = first_part(rest) {
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
    fn the_cut_leaves_the_first_page_or_the_first_line_as_it_is() {
        // Nine languages: the first page, of one or two lines.
        for shape in [&[1, 2][..], &[2, 2], &[2, 3], &[3, 1]] {
            let text = message(shape);
            let end = cut(&text);
            assert_eq!(text[..end], message(&shape[..1]));
        }
        // One page: its first line.
        assert_eq!(cut(&message(&[2])), 4);
        assert_eq!(cut(&message(&[1])), 4);
        assert_eq!(cut(&[]), 0);
    }

    #[test]
    fn the_error_screen_is_the_reason_as_it_is_and_then_the_home_line_with_the_numbers() {
        // The HOME line is one line: the numbers go two lines under it.
        let mut home = [0xffff; 64];
        home[..4].copy_from_slice(&message(&[1]));
        let label = Label::new(b"v1.2.3", b"-test").unwrap();
        let home_len = put(&mut home, 4, label.units(), 3, 0x7a00_0201).unwrap();
        // Bank's failure message, cut after its first page.
        let reason = message(&[2, 3]);
        let mut buffer = vec![0xffff; 128];
        buffer[..reason.len()].copy_from_slice(&reason);
        let len = cut(&reason);
        let len = append_page(&mut buffer, len, &home[..home_len]).unwrap();
        let all: Vec<String> = pages(&buffer[..len])
            .iter()
            .map(|page| String::from_utf16(page).unwrap())
            .collect();
        assert_eq!(all, ["text\ntext", "text\nv1.2.3-test\n00000003 7A000201"]);
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

    #[test]
    fn other_control_codes_are_stepped_over() {
        // A value of the message, as "{name}" in the original's texts: its
        // arguments are not text, whatever they hold.
        let mut text: Vec<u16> = "a".encode_utf16().collect();
        text.extend([CONTROL, 2, 0x0100, LINE_FEED]);
        text.extend("b".encode_utf16());
        text.extend(PAGE);
        text.extend("c".encode_utf16());
        assert_eq!(first_part(&text), (Some(6), None));
        // A code cut off at the end of the text ends the search.
        assert_eq!(first_part(&[CONTROL, 1]), (None, None));
    }
}
