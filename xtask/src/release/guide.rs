//! The guides, written once and shown twice: as Markdown in the release
//! notes and as plain text in the README.txt of each zip. A block is a
//! paragraph, lines without a blank between them, or a step with a picture
//! or a table. `plain` gives other words for the zip where the page links
//! within itself; a table row's fourth cell does so for its third.

pub enum Block {
    P {
        text: String,
        plain: Option<String>,
    },
    Lines(Vec<String>),
    Step {
        text: String,
        plain: Option<String>,
        picture: Option<String>,
        table: Option<Table>,
    },
}

impl Block {
    pub fn p(text: impl Into<String>) -> Block {
        Block::P {
            text: text.into(),
            plain: None,
        }
    }

    pub fn step(text: impl Into<String>) -> Block {
        Block::Step {
            text: text.into(),
            plain: None,
            picture: None,
            table: None,
        }
    }

    /// A step with other words in the README.
    pub fn step_plain(text: impl Into<String>, plain: impl Into<String>) -> Block {
        Block::Step {
            text: text.into(),
            plain: Some(plain.into()),
            picture: None,
            table: None,
        }
    }

    pub fn step_picture(text: impl Into<String>, picture: impl Into<String>) -> Block {
        Block::Step {
            text: text.into(),
            plain: None,
            picture: Some(picture.into()),
            table: None,
        }
    }

    pub fn step_table(text: impl Into<String>, table: Table) -> Block {
        Block::Step {
            text: text.into(),
            plain: None,
            picture: None,
            table: Some(table),
        }
    }
}

/// Rows of three cells or more: the README shows the second and the fourth
/// cell, or the second and the third.
pub struct Table {
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A row of cells, each a `&str` or a `String`.
macro_rules! row {
    ($($cell:expr),* $(,)?) => {
        vec![$(AsRef::<str>::as_ref(&$cell).to_owned()),*]
    };
}
pub(crate) use row;

/// GitHub's anchor of a heading.
pub fn anchor(heading: &str) -> String {
    let kept: String = heading
        .to_lowercase()
        .chars()
        .filter(|&c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        .collect();
    format!("#{}", kept.replace(' ', "-"))
}

/// `[label](target)` at `at` in `text`: the label and where it ends.
fn link(text: &str, at: usize) -> Option<(&str, usize)> {
    let rest = text.get(at..)?.strip_prefix('[')?;
    let close = rest.find(']')?;
    let after = rest[close + 1..].strip_prefix('(')?;
    let end = after.find(')')?;
    let length = 1 + close + 1 + 1 + end + 1;
    Some((&rest[..close], at + length))
}

/// Markdown as the README.txt says it: without pictures, links, marks and
/// accents.
pub fn plain(text: &str) -> String {
    let mut without_pictures = String::new();
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with('!') {
            if let Some((_, end)) = link(text, at + 1) {
                at = end;
                continue;
            }
        }
        let c = text[at..].chars().next().expect("in the text");
        without_pictures.push(c);
        at += c.len_utf8();
    }
    let text = without_pictures;
    let mut out = String::new();
    let mut at = 0;
    while at < text.len() {
        if let Some((label, end)) = link(&text, at) {
            out.push_str(label);
            at = end;
            continue;
        }
        let c = text[at..].chars().next().expect("in the text");
        out.push(c);
        at += c.len_utf8();
    }
    out.replace("**", "").replace('`', "").replace('é', "e")
}

/// Wraps at 76 columns, the first line after `first` and the others after
/// `rest`.
pub fn wrap(text: &str, first: &str, rest: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = first.to_owned();
    let mut start = true;
    for word in text.split(' ') {
        if start {
            line.push_str(word);
            start = false;
        } else if line.chars().count() + 1 + word.chars().count() > 76 {
            lines.push(std::mem::replace(&mut line, format!("{rest}{word}")));
        } else {
            line.push(' ');
            line.push_str(word);
        }
    }
    lines.push(line);
    lines
}

pub fn markdown_table(table: &Table, indent: &str) -> Vec<String> {
    let columns = table.head.len();
    let mut out = vec![
        format!("{indent}| {} |", table.head.join(" | ")),
        format!("{indent}|{}", " --- |".repeat(columns)),
    ];
    for row in &table.rows {
        out.push(format!("{indent}| {} |", row[..columns].join(" | ")));
    }
    out
}

/// The first cell as a column as wide as its longest entry up to 24
/// characters; a longer one stands on lines of its own.
pub fn text_table(table: &Table, indent: &str) -> Vec<String> {
    let width = table
        .rows
        .iter()
        .map(|row| plain(&row[0]).chars().count())
        .filter(|&length| length <= 24)
        .max()
        .unwrap_or(0);
    let rest = format!("{indent}{}", " ".repeat(width + 2));
    let mut out = Vec::new();
    for row in &table.rows {
        let key = plain(&row[0]);
        let what = match row.get(3) {
            Some(plain) => plain.clone(),
            None => plain(&row[2]),
        };
        let text = format!("{} {what}", plain(&row[1]));
        if key.chars().count() <= width {
            let first = format!("{indent}{key:<0$}", width + 2);
            out.extend(wrap(&text, &first, &rest));
        } else {
            out.extend(wrap(&key, indent, indent));
            out.extend(wrap(&text, &rest, &rest));
        }
    }
    out
}

/// Steps follow each other without a blank line unless one has a picture or
/// a table.
pub fn markdown(blocks: &[Block]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut step = 0;
    let mut tight = false;
    for block in blocks {
        let is_step = matches!(block, Block::Step { .. });
        if !out.is_empty() && !(tight && is_step) {
            out.push(String::new());
        }
        tight = false;
        match block {
            Block::Step {
                text,
                picture,
                table,
                ..
            } => {
                step += 1;
                out.push(format!("{step}. {text}"));
                if let Some(picture) = picture {
                    out.push(String::new());
                    out.push(format!("   {picture}"));
                } else if let Some(table) = table {
                    out.push(String::new());
                    out.extend(markdown_table(table, "   "));
                } else {
                    tight = true;
                }
            }
            Block::Lines(lines) => out.extend(lines.iter().cloned()),
            Block::P { text, .. } => out.push(text.clone()),
        }
    }
    out
}

pub fn text(blocks: &[Block]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut step = 0;
    let mut in_steps = false;
    for block in blocks {
        let is_step = matches!(block, Block::Step { .. });
        if !out.is_empty() && !(in_steps && is_step) {
            out.push(String::new());
        }
        in_steps = is_step;
        match block {
            Block::Step {
                text,
                plain: other,
                table,
                ..
            } => {
                step += 1;
                let words = other.clone().unwrap_or_else(|| plain(text));
                out.extend(wrap(&words, &format!("{step}. "), "   "));
                if let Some(table) = table {
                    out.extend(text_table(table, "     "));
                }
            }
            Block::Lines(lines) => {
                for line in lines {
                    out.extend(wrap(&plain(line), "", ""));
                }
            }
            Block::P { text, plain: other } => {
                let words = other.clone().unwrap_or_else(|| plain(text));
                out.extend(wrap(&words, "", ""));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_are_githubs() {
        assert_eq!(anchor("Installing v0.7.0"), "#installing-v070");
        assert_eq!(
            anchor("Pokémon still on the official server"),
            "#pokémon-still-on-the-official-server"
        );
    }

    #[test]
    fn plain_text_drops_marks_pictures_and_links() {
        assert_eq!(
            plain("**Only** `x.zip`, see [Checkpoint](https://a/b) ![shot](https://c/d.png)Pokémon [x] (y)"),
            "Only x.zip, see Checkpoint Pokemon [x] (y)"
        );
    }

    #[test]
    fn lines_wrap_at_76_columns() {
        let text = "word ".repeat(30);
        let lines = wrap(text.trim_end(), "1. ", "   ");
        assert!(lines.iter().all(|l| l.chars().count() <= 76));
        assert!(lines[0].starts_with("1. word"));
        assert!(lines[1].starts_with("   word"));
        assert_eq!(wrap("", "- ", "  "), ["- "]);
    }

    #[test]
    fn tables_show_both_ways() {
        let table = Table {
            head: row!["You see", "Meaning", "What to do"],
            rows: vec![
                row![
                    "`00000001`",
                    "Converted.",
                    "Go on with [step 6](#six).",
                    "Go on."
                ],
                row![
                    "A key that is much longer than twenty-four",
                    "Meaning.",
                    "Do it."
                ],
            ],
        };
        assert_eq!(
            markdown_table(&table, ""),
            [
                "| You see | Meaning | What to do |",
                "| --- | --- | --- |",
                "| `00000001` | Converted. | Go on with [step 6](#six). |",
                "| A key that is much longer than twenty-four | Meaning. | Do it. |",
            ]
        );
        assert_eq!(
            text_table(&table, "  "),
            [
                "  00000001  Converted. Go on.",
                "  A key that is much longer than twenty-four",
                "            Meaning. Do it.",
            ]
        );
    }

    #[test]
    fn steps_are_numbered_and_spaced() {
        let blocks = [
            Block::p("Intro."),
            Block::step("One."),
            Block::step("Two."),
            Block::step_picture("Three.", "![a](b)"),
            Block::step_plain("Four [here](#x).", "Four below."),
        ];
        assert_eq!(
            markdown(&blocks),
            [
                "Intro.",
                "",
                "1. One.",
                "2. Two.",
                "3. Three.",
                "",
                "   ![a](b)",
                "",
                "4. Four [here](#x)."
            ]
        );
        assert_eq!(
            text(&blocks),
            [
                "Intro.",
                "",
                "1. One.",
                "2. Two.",
                "3. Three.",
                "4. Four below."
            ]
        );
    }
}
