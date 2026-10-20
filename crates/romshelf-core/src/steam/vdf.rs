//! Steam's binary KeyValues format, as `shortcuts.vdf` uses it.
//!
//! Every item is a type byte, a NUL-terminated key, then the value: `0x00` a nested map (items
//! until `0x08`), `0x01` a NUL-terminated string, `0x02` a little-endian 32-bit integer. A map
//! ends with `0x08`, and so does the file.
//!
//! The tree keeps every key, value and the order they came in, so a file read and written back
//! is byte for byte the same, including fields newer than any list of known ones
//! (`FlatpakAppID`, `sortas`, seen on the Deck in 2026).

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Map(Map),
    /// Raw bytes: Steam writes UTF-8, but nothing here depends on it.
    Str(Vec<u8>),
    Int(u32),
}

/// Keys and values in file order.
pub type Map = Vec<(Vec<u8>, Value)>;

const MAP: u8 = 0x00;
const STR: u8 = 0x01;
const INT: u8 = 0x02;
const END: u8 = 0x08;

fn bad(at: usize, what: &str) -> Error {
    Error::Invalid(format!(
        "Steam's shortcuts file isn't in the expected format ({what} at byte {at}), so Romshelf left it alone."
    ))
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| bad(self.at, "unexpected end"))?;
        self.at += 1;
        Ok(b)
    }

    fn cstr(&mut self) -> Result<Vec<u8>> {
        let rest = &self.bytes[self.at..];
        let len = rest
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| bad(self.at, "unterminated text"))?;
        let s = rest[..len].to_vec();
        self.at += len + 1;
        Ok(s)
    }

    fn map(&mut self) -> Result<Map> {
        let mut out = Map::new();
        loop {
            let t = self.byte()?;
            if t == END {
                return Ok(out);
            }
            let key = self.cstr()?;
            let value = match t {
                MAP => Value::Map(self.map()?),
                STR => Value::Str(self.cstr()?),
                INT => {
                    let raw = self
                        .bytes
                        .get(self.at..self.at + 4)
                        .ok_or_else(|| bad(self.at, "short number"))?;
                    self.at += 4;
                    Value::Int(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
                }
                other => return Err(bad(self.at - 1, &format!("unknown type {other}"))),
            };
            out.push((key, value));
        }
    }
}

/// Reads a whole file: the root map's items, then the closing `0x08`.
pub fn parse(bytes: &[u8]) -> Result<Map> {
    let mut r = Reader { bytes, at: 0 };
    let root = r.map()?;
    if r.at != bytes.len() {
        return Err(bad(r.at, "extra data after the end"));
    }
    Ok(root)
}

fn write_map(map: &Map, out: &mut Vec<u8>) {
    for (key, value) in map {
        let t = match value {
            Value::Map(_) => MAP,
            Value::Str(_) => STR,
            Value::Int(_) => INT,
        };
        out.push(t);
        out.extend_from_slice(key);
        out.push(0);
        match value {
            Value::Map(m) => write_map(m, out),
            Value::Str(s) => {
                out.extend_from_slice(s);
                out.push(0);
            }
            Value::Int(n) => out.extend_from_slice(&n.to_le_bytes()),
        }
    }
    out.push(END);
}

pub fn write(root: &Map) -> Vec<u8> {
    let mut out = Vec::new();
    write_map(root, &mut out);
    out
}

/// The value under `key`, if any.
pub fn get<'m>(map: &'m Map, key: &str) -> Option<&'m Value> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes()))
        .map(|(_, v)| v)
}

/// Sets `key` in place, keeping its position; a new key goes at the end.
pub fn set(map: &mut Map, key: &str, value: Value) {
    match map
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes()))
    {
        Some((_, v)) => *v = value,
        None => map.push((key.as_bytes().to_vec(), value)),
    }
}

pub fn str_of(map: &Map, key: &str) -> Option<String> {
    match get(map, key) {
        Some(Value::Str(s)) => Some(String::from_utf8_lossy(s).into_owned()),
        _ => None,
    }
}

pub fn int_of(map: &Map, key: &str) -> Option<u32> {
    match get(map, key) {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_file_round_trips() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x00shortcuts\x00\x000\x00\x02appid\x00\x01\x02\x03\x84");
        bytes.extend_from_slice(
            b"\x01AppName\x00Caf\xc3\xa9\x00\x00tags\x00\x010\x00Romshelf\x00\x08\x08\x08\x08",
        );
        let root = parse(&bytes).unwrap();
        assert_eq!(write(&root), bytes);
        let Value::Map(shortcuts) = get(&root, "shortcuts").unwrap() else {
            panic!()
        };
        let Value::Map(entry) = &shortcuts[0].1 else {
            panic!()
        };
        assert_eq!(int_of(entry, "appid"), Some(0x8403_0201));
        assert_eq!(str_of(entry, "appname").as_deref(), Some("Café"));
    }

    #[test]
    fn damage_is_refused_not_guessed() {
        assert!(parse(b"\x00shortcuts\x00").is_err());
        assert!(parse(b"\x05x\x00\x08").is_err());
        assert!(parse(b"\x08\x08").is_err());
        assert!(parse(b"\x01name").is_err());
    }
}
