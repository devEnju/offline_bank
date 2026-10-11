//! The line of two numbers that Bank and Poké Transporter put into a message
//! of the original, and where it goes.
//!
//! A message is the original's own text in UTF-16 units. A line feed ends a
//! line; a control code is the unit `0x10`, the number of units that follow,
//! then the code and its arguments; the code `0xBE01` ends a page (wait for a
//! button, then clear). The dialog holds three lines, and the numbers are
//! always its third: only a first page of one or two lines gets them.

/// A control code in the original's texts.
const CONTROL: u16 = 0x10;
/// The control code between two pages.
const NEW_PAGE: u16 = 0xbe01;
const LINE_FEED: u16 = 0x0a;
/// Two numbers of eight digits with a space between them.
pub const LEN: usize = 17;
/// The dialog's last line.
const LINE: usize = 3;

/// What becomes of the message behind the numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rest {
    /// The message ends with the numbers. Its first part is its first page
    /// or, in a message of one page, its first line.
    Dropped,
    /// The pages after the first follow as they are.
    Kept,
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

/// Puts the numbers on the third line of the first page of the message in
/// `text[..len]`. `text` is the whole room of the string, and the unit after
/// the message stays free for its end mark. Answers the new length, or `None`
/// with the message untouched: its first page has three lines already, or
/// the room is too small.
pub fn put(text: &mut [u16], len: usize, first: u32, second: u32, rest: Rest) -> Option<usize> {
    if len > text.len() {
        return None;
    }
    let (page, feed) = first_part(&text[..len]);
    let end = match (rest, page) {
        (_, Some(page)) => page,
        (Rest::Dropped, None) => feed.unwrap_or(len),
        (Rest::Kept, None) => len,
    };
    let lines = 1 + text[..end]
        .iter()
        .filter(|&&unit| unit == LINE_FEED)
        .count();
    if lines >= LINE {
        return None;
    }
    let feeds = LINE - lines;
    let added = feeds + LEN;
    let tail = match rest {
        Rest::Dropped => 0,
        Rest::Kept => len - end,
    };
    let new_len = end + added + tail;
    if new_len >= text.len() {
        return None;
    }
    text.copy_within(end..end + tail, end + added);
    text[end..end + feeds].fill(LINE_FEED);
    text[end + feeds..end + added].copy_from_slice(&line(first, second));
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
    fn put_in(text: &[u16], room: usize, rest: Rest) -> Option<Vec<u16>> {
        let mut buffer = vec![0xffff; room];
        buffer[..text.len()].copy_from_slice(text);
        let len = put(&mut buffer, text.len(), 3, 0x7a00_0201, rest)?;
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
    fn the_numbers_are_the_third_line_of_the_first_page() {
        // One line: an empty line between, as under the HOME line.
        for rest in [Rest::Dropped, Rest::Kept] {
            let out = put_in(&message(&[1]), 64, rest).unwrap();
            assert_eq!(first_page(&out), ["text", "", "00000003 7A000201"]);
        }
        // Two lines: directly under them.
        let out = put_in(&message(&[2, 3]), 96, Rest::Kept).unwrap();
        assert_eq!(first_page(&out), ["text", "text", "00000003 7A000201"]);
    }

    #[test]
    fn a_kept_message_goes_on_with_its_later_pages_unchanged() {
        // Poké Transporter's "open Pokémon Bank" message in the ten
        // languages: two lines on its first page in all of them, two or
        // three on its second.
        for second in [3, 3, 2, 2, 3, 2, 2, 3, 3, 3] {
            let text = message(&[2, second]);
            let page = first_part(&text).0.unwrap();
            let out = put_in(&text, 128, Rest::Kept).unwrap();
            assert_eq!(first_page(&out), ["text", "text", "00000003 7A000201"]);
            let moved = first_part(&out).0.unwrap();
            assert_eq!(moved, page + 1 + LEN);
            assert_eq!(out[moved..], text[page..]);
        }
    }

    #[test]
    fn a_dropped_message_ends_with_the_numbers() {
        // Bank's failure message: its first page has one or two lines.
        for first in [1, 2] {
            let out = put_in(&message(&[first, 2]), 96, Rest::Dropped).unwrap();
            assert_eq!(first_part(&out).0, None);
            assert_eq!(out[out.len() - LEN..], numbers());
            assert_eq!(first_page(&out).len(), 3);
        }
        // One language has it on one page, its first sentence on one line.
        let out = put_in(&message(&[2]), 64, Rest::Dropped).unwrap();
        assert_eq!(first_page(&out), ["text", "", "00000003 7A000201"]);
    }

    #[test]
    fn a_first_page_of_three_lines_is_left_alone() {
        for rest in [Rest::Dropped, Rest::Kept] {
            assert_eq!(put_in(&message(&[3, 3]), 128, rest), None);
            assert_eq!(put_in(&message(&[4, 1]), 128, rest), None);
        }
        // Kept, a message of one page is that page as a whole.
        assert_eq!(put_in(&message(&[3]), 128, Rest::Kept), None);
        assert_eq!(put_in(&message(&[4]), 128, Rest::Kept), None);
    }

    #[test]
    fn without_room_nothing_is_written() {
        let text = message(&[2, 2]);
        for rest in [Rest::Dropped, Rest::Kept] {
            let needed = put_in(&text, 128, rest).unwrap().len();
            // One unit stays free for the end mark.
            assert!(put_in(&text, needed + 1, rest).is_some());
            let mut buffer = vec![0xffff; needed];
            buffer[..text.len()].copy_from_slice(&text);
            assert_eq!(put(&mut buffer, text.len(), 1, 2, rest), None);
            assert_eq!(buffer[..text.len()], text);
        }
        assert_eq!(put(&mut [0; 4], 5, 1, 2, Rest::Kept), None);
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
