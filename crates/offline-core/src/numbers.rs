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

/// Ends the message in `text[..len]` after its first part and puts the
/// numbers on the third line there. The first part is the first page or, in
/// a message of one page, its first line. `text` is the whole room of the
/// string, and the unit after the message stays free for its end mark.
/// Answers the new length, or `None` with the message untouched: its first
/// page has three lines already, or the room is too small.
pub fn put(text: &mut [u16], len: usize, first: u32, second: u32) -> Option<usize> {
    if len > text.len() {
        return None;
    }
    let (page, feed) = first_part(&text[..len]);
    let end = page.or(feed).unwrap_or(len);
    let lines = 1 + text[..end]
        .iter()
        .filter(|&&unit| unit == LINE_FEED)
        .count();
    if lines >= LINE {
        return None;
    }
    let feeds = LINE - lines;
    let new_len = end + feeds + LEN;
    if new_len >= text.len() {
        return None;
    }
    text[end..end + feeds].fill(LINE_FEED);
    text[end + feeds..new_len].copy_from_slice(&line(first, second));
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
        let len = put(&mut buffer, text.len(), 3, 0x7a00_0201)?;
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
        assert_eq!(put(&mut buffer, text.len(), 1, 2), None);
        assert_eq!(buffer[..text.len()], text);
        assert_eq!(put(&mut [0; 4], 5, 1, 2), None);
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
