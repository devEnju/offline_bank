//! Where a message of the original ends its first part and where it goes
//! on: the walk over its text that every patch's dialogs use.
//!
//! A message is the original's own text in UTF-16 units. A line feed ends a
//! line; a control code is the unit `0x10`, the number of units that follow,
//! then the code and its arguments; the code `0xBE01` ends a page (wait for a
//! button, then clear). Bank and Transporter write their messages the same
//! way.

/// A control code in the original's texts.
pub const CONTROL: u16 = 0x10;
/// The control code between two pages.
pub const NEW_PAGE: u16 = 0xbe01;
pub const LINE_FEED: u16 = 0x0a;

/// End of `pages` pages, immediately before the next page-break command.
/// Control arguments are skipped as records, never interpreted as text.
/// A missing boundary or malformed record is an error, not the full message.
pub fn page_end(text: &[u16], pages: usize) -> Option<usize> {
    boundaries(text, pages).0
}

/// Where the first part of the message `text` ends: its first page or, in
/// a message of one page, its first line. Nothing is written; a message
/// that ends there is the original's own text up to that point.
pub fn cut(text: &[u16]) -> usize {
    let (page, feed) = boundaries(text, 1);
    page.or(feed).unwrap_or(text.len())
}

/// Where the first line of the message `text` ends: at its first line feed,
/// or at its first page break if that comes before one. Nothing is written.
/// For a message of one page whose first line is a sentence of its own, as
/// 26:0x00 "The SD card is full." is in every language of the original.
pub fn first_line(text: &[u16]) -> usize {
    let (page, feed) = boundaries(text, 1);
    feed.or(page).unwrap_or(text.len())
}

/// Where the message `text` goes on after its first part (`cut`): behind
/// the page break and the line feed the next page begins with or, in a
/// message of one page, behind its first line. `None` if nothing follows.
pub fn rest(text: &[u16]) -> Option<usize> {
    let start = match boundaries(text, 1) {
        (Some(page), _) => {
            let start = page + 3;
            start + usize::from(text.get(start) == Some(&LINE_FEED))
        }
        (None, feed) => feed? + 1,
    };
    text.get(start)
        .is_some_and(|&unit| unit != 0)
        .then_some(start)
}

/// Keeps only what follows the first part (`rest`) of the message in
/// `room[..len]`: moves it to the front and ends it there. `room` is the
/// whole room of the string, and the unit after the message has to be free
/// for its end mark. Answers the new length, or `None` with the message
/// untouched if nothing follows its first part or it does not fit.
pub fn keep_rest(room: &mut [u16], len: usize) -> Option<usize> {
    if len >= room.len() {
        return None;
    }
    let start = rest(&room[..len])?;
    room.copy_within(start..len, 0);
    let new_len = len - start;
    room[new_len] = 0;
    Some(new_len)
}

/// The requested page break and first line feed outside control records.
/// Both dialog cuts use this walk; a malformed record ends the search.
fn boundaries(text: &[u16], mut pages: usize) -> (Option<usize>, Option<usize>) {
    if pages == 0 {
        return (None, None);
    }
    let mut line = None;
    let mut at = 0;
    while at < text.len() {
        if text[at] == 0 {
            break;
        }
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
        let count = usize::from(text[at + 1]);
        let end = at + 2 + count;
        if count == 0 || end > text.len() {
            break;
        }
        if text[at + 2] == NEW_PAGE {
            if count != 1 {
                break;
            }
            pages -= 1;
            if pages == 0 {
                return (Some(at), line);
            }
        }
        at = end;
    }
    (None, line)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

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

    #[test]
    fn page_prefix_keeps_the_first_break_and_ends_before_the_second() {
        let text = message(&[2, 2, 2]);
        let end = page_end(&text, 2).unwrap();
        assert_eq!(&text[..end], message(&[2, 2]));
        assert_eq!(&text[end..end + 3], PAGE);
        assert_eq!(page_end(&text[..end], 2), None);
        assert_eq!(page_end(&text, 1), Some(message(&[2]).len()));
    }

    #[test]
    fn page_boundaries_skip_control_arguments_and_reject_malformed_records() {
        let mut text = vec![CONTROL, 4, 0x0100, CONTROL, 1, NEW_PAGE];
        text.extend(message(&[1, 1, 1]));
        assert_eq!(cut(&text), 6 + message(&[1]).len());
        assert_eq!(page_end(&text, 2), Some(6 + message(&[1, 1]).len()));
        for malformed in [
            vec![],
            vec![CONTROL],
            vec![CONTROL, 0],
            vec![CONTROL, 2, NEW_PAGE],
            vec![CONTROL, 2, NEW_PAGE, 7],
            vec![0, CONTROL, 1, NEW_PAGE],
        ] {
            assert_eq!(page_end(&malformed, 1), None);
        }
        let text = message(&[1, 1, 1]);
        assert_eq!(page_end(&text, 0), None);
        assert_eq!(page_end(&text, 3), None);
        let end = page_end(&text, 2).unwrap();
        for length in 0..end + PAGE.len() {
            assert_eq!(page_end(&text[..length], 2), None);
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
    fn the_first_line_ends_at_the_first_line_feed_or_page_break() {
        // One page of two lines, as 26:0x00: its first line.
        assert_eq!(first_line(&message(&[2])), 4);
        // A first page of one line ends at the page break.
        assert_eq!(first_line(&message(&[1, 2])), 4);
        // Two lines on the first page: the first of them.
        assert_eq!(first_line(&message(&[2, 1])), 4);
        // One line alone is the whole message.
        assert_eq!(first_line(&message(&[1])), 4);
        assert_eq!(first_line(&[]), 0);
        // A line feed inside a control record is no line's end.
        let mut text: Vec<u16> = "a".encode_utf16().collect();
        text.extend([CONTROL, 2, 0x0100, LINE_FEED]);
        text.extend([b'b' as u16, LINE_FEED, b'c' as u16]);
        assert_eq!(first_line(&text), 6);
    }

    #[test]
    fn the_rest_is_what_follows_the_first_page_or_the_first_line() {
        // Nine languages: the pages after the first, as the original has them.
        for shape in [&[1, 2][..], &[2, 2], &[2, 3], &[1, 3, 2]] {
            let text = message(shape);
            let start = rest(&text).unwrap();
            assert_eq!(text[start..], message(&shape[1..]));
        }
        // One page: what follows its first line.
        let text = message(&[2]);
        assert_eq!(rest(&text), Some(5));
        assert_eq!(text[5..], message(&[1]));
        // Nothing follows, or the page break is not a record.
        for nothing in [
            message(&[1]),
            vec![],
            [message(&[1]), PAGE.to_vec()].concat(),
            [message(&[1]), PAGE.to_vec(), vec![LINE_FEED]].concat(),
            [message(&[1]), vec![LINE_FEED, 0]].concat(),
            vec![CONTROL, 0, LINE_FEED],
        ] {
            assert_eq!(rest(&nothing), None, "{nothing:x?}");
        }
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
        assert_eq!(boundaries(&text, 1), (Some(6), None));
        assert_eq!(cut(&text), 6);
        // The one-page error's fallback also ignores line feeds in arguments.
        let mut single = text[..6].to_vec();
        single.extend([LINE_FEED, b'c' as u16]);
        assert_eq!(cut(&single), 6);
        assert_eq!(page_end(&single, 1), None);
        // A code cut off at the end of the text ends the search.
        assert_eq!(boundaries(&[CONTROL, 1], 1), (None, None));
    }

    #[test]
    fn only_the_rest_is_kept_and_ended_where_it_now_ends() {
        // Two pages: the second, without the line feed it begins with.
        let text = message(&[1, 2]);
        let mut room = vec![0xffff; 64];
        room[..text.len()].copy_from_slice(&text);
        let len = keep_rest(&mut room, text.len()).unwrap();
        assert_eq!(room[..len], message(&[2]));
        assert_eq!(room[len], 0);
        // One page: what follows its first line.
        let text = message(&[2]);
        let mut room = vec![0xffff; 16];
        room[..text.len()].copy_from_slice(&text);
        assert_eq!(keep_rest(&mut room, text.len()), Some(4));
        assert_eq!(
            room[..5],
            [b't' as u16, b'e' as u16, b'x' as u16, b't' as u16, 0]
        );
        // Nothing follows, or no unit is free for the end mark: untouched.
        let text = message(&[1]);
        let mut room = text.clone();
        room.push(0xffff);
        assert_eq!(keep_rest(&mut room, text.len()), None);
        assert_eq!(room[..text.len()], text);
        let text = message(&[1, 1]);
        let mut room = text.clone();
        assert_eq!(keep_rest(&mut room, text.len()), None);
        assert_eq!(room, text);
    }
}
