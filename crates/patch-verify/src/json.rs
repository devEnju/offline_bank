//! JSON as the builder, cargo and this crate write it: a reader of whole
//! documents and a writer with two-space indents. Numbers are kept as
//! written and read as integers on demand; members keep their order, and a
//! member named twice is refused, so that no reader of a manifest can see
//! another value than this one.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A number, as written.
    Number(String),
    String(String),
    Array(Vec<Json>),
    /// Members in the order they are written.
    Object(Vec<(String, Json)>),
}

/// How deep arrays and objects may nest.
const DEPTH: usize = 64;

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            text: text.as_bytes(),
            at: 0,
        };
        reader.space();
        let value = reader.value(0)?;
        reader.space();
        if reader.at != reader.text.len() {
            return Err(reader.error("text after the document"));
        }
        Ok(value)
    }

    pub fn number(value: u64) -> Json {
        Json::Number(value.to_string())
    }

    pub fn string(value: impl Into<String>) -> Json {
        Json::String(value.into())
    }

    pub fn object<K: Into<String>>(members: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Object(members.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Object(members) => members.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The members of an object, in order.
    pub fn members(&self) -> &[(String, Json)] {
        match self {
            Json::Object(members) => members,
            _ => &[],
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// A number written as a whole number that fits.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(text) if text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok(),
            _ => None,
        }
    }

    /// The document with two-space indents and a final line break.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        let pad = |out: &mut String, depth: usize| out.extend((0..depth * 2).map(|_| ' '));
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Number(text) => out.push_str(text),
            Json::String(text) => quote(out, text),
            Json::Array(items) if items.is_empty() => out.push_str("[]"),
            Json::Object(members) if members.is_empty() => out.push_str("{}"),
            Json::Array(items) => {
                out.push_str("[\n");
                for (index, item) in items.iter().enumerate() {
                    pad(out, indent + 1);
                    item.write(out, indent + 1);
                    out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
                }
                pad(out, indent);
                out.push(']');
            }
            Json::Object(members) => {
                out.push_str("{\n");
                for (index, (key, value)) in members.iter().enumerate() {
                    pad(out, indent + 1);
                    quote(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    out.push_str(if index + 1 < members.len() {
                        ",\n"
                    } else {
                        "\n"
                    });
                }
                pad(out, indent);
                out.push('}');
            }
        }
    }
}

fn quote(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn error(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.at)
    }

    fn space(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unknown word"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        if depth == DEPTH {
            return Err(self.error("nested too deep"));
        }
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            if members.iter().any(|(k, _)| *k == key) {
                return Err(self.error(&format!("member \"{key}\" named twice")));
            }
            self.space();
            self.expect(b':')?;
            self.space();
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        if depth == DEPTH {
            return Err(self.error("nested too deep"));
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        let digits = |reader: &mut Self| {
            let from = reader.at;
            while matches!(reader.peek(), Some(b'0'..=b'9')) {
                reader.at += 1;
            }
            reader.at > from
        };
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else if !digits(self) {
            return Err(self.error("number without digits"));
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !digits(self) {
                return Err(self.error("fraction without digits"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                return Err(self.error("exponent without digits"));
            }
        }
        // Only ASCII was consumed.
        let text = std::str::from_utf8(&self.text[start..self.at]).expect("ASCII");
        Ok(Json::Number(text.to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.error("bad \\u escape"))?;
        self.at += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.at += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&first) {
                                if !self.text[self.at..].starts_with(b"\\u") {
                                    return Err(self.error("lone surrogate"));
                                }
                                self.at += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&second) {
                                    return Err(self.error("lone surrogate"));
                                }
                                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                            } else {
                                first
                            };
                            char::from_u32(code).ok_or_else(|| self.error("lone surrogate"))?
                        }
                        _ => return Err(self.error("unknown escape")),
                    };
                    let mut buffer = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                0..=0x1f => return Err(self.error("control character in string")),
                _ => out.push(byte),
            }
        }
        String::from_utf8(out).map_err(|_| self.error("string is not UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn a_manifest_reads_and_writes_back_as_written() {
        let text = "{\n  \"patch\": \"bank\",\n  \"bytes\": 2048,\n  \"artifacts\": {\n    \"code.ips\": {\n      \"sha256\": \"ab\"\n    }\n  },\n  \"checks\": [\n    \"one\",\n    \"two\"\n  ],\n  \"empty\": [],\n  \"none\": {},\n  \"tested\": false,\n  \"nothing\": null\n}\n";
        let json = Json::parse(text).unwrap();
        assert_eq!(json.get("patch").and_then(Json::as_str), Some("bank"));
        assert_eq!(json.get("bytes").and_then(Json::as_u64), Some(2048));
        assert_eq!(
            json.get("artifacts")
                .and_then(|a| a.get("code.ips"))
                .and_then(|c| c.get("sha256"))
                .and_then(Json::as_str),
            Some("ab")
        );
        assert_eq!(json.get("tested").and_then(Json::as_bool), Some(false));
        assert_eq!(json.to_pretty(), text);
    }

    #[test]
    fn the_builders_inline_objects_read() {
        let json = Json::parse("{ \"a\": { \"sha256\": \"x\", \"bytes\": 1 } }").unwrap();
        assert_eq!(
            json.get("a")
                .and_then(|a| a.get("bytes"))
                .and_then(Json::as_u64),
            Some(1)
        );
    }

    #[test]
    fn strings_escape_and_unescape() {
        let text = "a\"b\\c\nd\u{1}é😀";
        let json = Json::string(text);
        let written = json.to_pretty();
        assert_eq!(written, "\"a\\\"b\\\\c\\nd\\u0001é😀\"\n");
        assert_eq!(Json::parse(&written).unwrap(), json);
        assert_eq!(
            Json::parse("\"\\u00e9\\ud83d\\ude00\\/\"").unwrap(),
            Json::string("é😀/")
        );
    }

    #[test]
    fn numbers_are_kept_as_written_and_read_as_whole_numbers() {
        let json = Json::parse("[0, -1, 1.5, 2e3, 4294967296]").unwrap();
        let Json::Array(items) = json else { panic!() };
        assert_eq!(items[0].as_u64(), Some(0));
        assert_eq!(items[1].as_u64(), None);
        assert_eq!(items[2].as_u64(), None);
        assert_eq!(items[3], Json::Number("2e3".into()));
        assert_eq!(items[4].as_u64(), Some(4_294_967_296));
    }

    #[test]
    fn malformed_documents_are_refused() {
        for text in [
            "",
            "{",
            "{\"a\": 1,}",
            "[1 2]",
            "{\"a\": 1, \"a\": 2}",
            "\"open",
            "\"\\x\"",
            "\"\\ud800\"",
            "01",
            "-",
            "1.",
            "tru",
            "{} {}",
            "\"tab\there\"",
        ] {
            assert!(Json::parse(text).is_err(), "accepted {text:?}");
        }
        let deep = "[".repeat(65) + &"]".repeat(65);
        assert!(Json::parse(&deep).is_err());
        let fine = "[".repeat(64) + &"]".repeat(64);
        assert!(Json::parse(&fine).is_ok());
    }

    #[test]
    fn members_can_be_changed() {
        let mut json = Json::object([("n", Json::number(1))]);
        *json.get_mut("n").unwrap() = Json::number(2);
        assert_eq!(json.to_pretty(), "{\n  \"n\": 2\n}\n");
    }
}
