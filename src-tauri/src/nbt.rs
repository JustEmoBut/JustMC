//! Just enough NBT to read and write `servers.dat`.
//!
//! Minecraft's tag format, uncompressed and big-endian. The launcher only
//! needs one file in it, but that file has to survive a round trip: the game
//! writes keys this launcher has never heard of (`acceptTextures`, whatever a
//! future version adds), and rebuilding the file from the fields we understand
//! would silently drop them. So the parse keeps the whole tree, order
//! included, and the writer puts back exactly what it read.
//!
//! Only what `servers.dat` can contain is implemented, which is all of it —
//! the tag set is small and stable.

use crate::error::{Error, Result};

const END: u8 = 0;
const BYTE: u8 = 1;
const SHORT: u8 = 2;
const INT: u8 = 3;
const LONG: u8 = 4;
const FLOAT: u8 = 5;
const DOUBLE: u8 = 6;
const BYTE_ARRAY: u8 = 7;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;
const LONG_ARRAY: u8 = 12;

/// One NBT value. `Compound` keeps its entries in file order rather than in a
/// map, because writing the file back in a different order is a needless diff
/// in something the game also writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// The element type is carried so an empty list writes back as the same
    /// kind of empty list it was.
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => BYTE,
            Tag::Short(_) => SHORT,
            Tag::Int(_) => INT,
            Tag::Long(_) => LONG,
            Tag::Float(_) => FLOAT,
            Tag::Double(_) => DOUBLE,
            Tag::ByteArray(_) => BYTE_ARRAY,
            Tag::String(_) => STRING,
            Tag::List(..) => LIST,
            Tag::Compound(_) => COMPOUND,
            Tag::IntArray(_) => INT_ARRAY,
            Tag::LongArray(_) => LONG_ARRAY,
        }
    }

    /// The value under a compound key, for the handful of lookups the servers
    /// file needs.
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    /// The mutable list under a compound key, creating it when the file has
    /// none yet — a fresh instance has no `servers` list until one is added.
    pub fn list_mut(&mut self, key: &str, element: u8) -> Option<&mut Vec<Tag>> {
        let Tag::Compound(entries) = self else { return None };
        if !entries.iter().any(|(k, _)| k == key) {
            entries.push((key.to_string(), Tag::List(element, Vec::new())));
        }
        match entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v) {
            Some(Tag::List(_, items)) => Some(items),
            _ => None,
        }
    }
}

/// A cursor over the bytes. Every read is bounds-checked: the file is on the
/// user's disk but a truncated or corrupt one must be an error, not a panic.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(|| Error::msg("NBT: length overflow"))?;
        let slice = self.bytes.get(self.at..end).ok_or_else(|| Error::msg("NBT: truncated"))?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        // Minecraft writes modified UTF-8; for the text these files hold it is
        // plain UTF-8, and a lossy decode beats failing the whole file.
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    /// The element count of a list or array, refused when negative or larger
    /// than the bytes left — a corrupt length must not preallocate gigabytes.
    fn count(&mut self, element_size: usize) -> Result<usize> {
        let len = self.i32()?;
        if len < 0 {
            return Ok(0); // the format's own way of saying "empty"
        }
        let len = len as usize;
        if element_size > 0 && len.saturating_mul(element_size) > self.bytes.len() - self.at {
            return Err(Error::msg("NBT: length runs past the end of the file"));
        }
        Ok(len)
    }

    fn payload(&mut self, id: u8) -> Result<Tag> {
        Ok(match id {
            BYTE => Tag::Byte(self.u8()? as i8),
            SHORT => Tag::Short(self.u16()? as i16),
            INT => Tag::Int(self.i32()?),
            LONG => Tag::Long(i64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            FLOAT => Tag::Float(f32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            DOUBLE => Tag::Double(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            BYTE_ARRAY => {
                let len = self.count(1)?;
                Tag::ByteArray(self.take(len)?.to_vec())
            }
            STRING => Tag::String(self.string()?),
            LIST => {
                let element = self.u8()?;
                // An element size of zero means "cannot be checked cheaply";
                // the per-element reads are bounds-checked anyway.
                let len = self.count(0)?;
                let mut items = Vec::new();
                for _ in 0..len {
                    if element == END {
                        break;
                    }
                    items.push(self.payload(element)?);
                }
                Tag::List(element, items)
            }
            COMPOUND => {
                let mut entries = Vec::new();
                loop {
                    let id = self.u8()?;
                    if id == END {
                        break;
                    }
                    let name = self.string()?;
                    entries.push((name, self.payload(id)?));
                }
                Tag::Compound(entries)
            }
            INT_ARRAY => {
                let len = self.count(4)?;
                let mut out = Vec::with_capacity(len);
                for _ in 0..len {
                    out.push(self.i32()?);
                }
                Tag::IntArray(out)
            }
            LONG_ARRAY => {
                let len = self.count(8)?;
                let mut out = Vec::with_capacity(len);
                for _ in 0..len {
                    out.push(i64::from_be_bytes(self.take(8)?.try_into().unwrap()));
                }
                Tag::LongArray(out)
            }
            other => return Err(Error::msg(format!("NBT: unknown tag {other}"))),
        })
    }
}

/// Parse a whole file, returning the root compound and the name it was stored
/// under (`servers.dat` uses an empty one, but it is written back as found).
pub fn read(bytes: &[u8]) -> Result<(String, Tag)> {
    let mut reader = Reader { bytes, at: 0 };
    let id = reader.u8()?;
    if id != COMPOUND {
        return Err(Error::msg("NBT: the root is not a compound"));
    }
    let name = reader.string()?;
    let root = reader.payload(COMPOUND)?;
    Ok((name, root))
}

fn write_string(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    // u16 is the format's limit; a name this long is not a real one.
    let len = bytes.len().min(u16::MAX as usize);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(&bytes[..len]);
}

fn write_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend_from_slice(v);
        }
        Tag::String(v) => write_string(out, v),
        Tag::List(element, items) => {
            // The declared element type has to match what is actually there,
            // or the game reads garbage; the items win.
            let element = items.first().map(Tag::id).unwrap_or(*element);
            out.push(element);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                write_payload(out, item);
            }
        }
        Tag::Compound(entries) => {
            for (name, value) in entries {
                out.push(value.id());
                write_string(out, name);
                write_payload(out, value);
            }
            out.push(END);
        }
        Tag::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for item in v {
                out.extend_from_slice(&item.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for item in v {
                out.extend_from_slice(&item.to_be_bytes());
            }
        }
    }
}

/// Serialise a root compound back to bytes.
pub fn write(name: &str, root: &Tag) -> Vec<u8> {
    let mut out = vec![COMPOUND];
    write_string(&mut out, name);
    write_payload(&mut out, root);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tag {
        Tag::Compound(vec![(
            "servers".to_string(),
            Tag::List(
                COMPOUND,
                vec![Tag::Compound(vec![
                    ("name".to_string(), Tag::String("Hypixel".into())),
                    ("ip".to_string(), Tag::String("mc.hypixel.net".into())),
                    // The key the launcher does not understand, and must keep.
                    ("acceptTextures".to_string(), Tag::Byte(1)),
                ])],
            ),
        )])
    }

    #[test]
    fn a_file_survives_a_round_trip_with_its_unknown_keys() {
        let bytes = write("", &sample());
        let (name, root) = read(&bytes).unwrap();
        assert_eq!(name, "");
        assert_eq!(root, sample());
        // Byte-for-byte, so writing the file back is not a gratuitous rewrite.
        assert_eq!(write(&name, &root), bytes);
    }

    #[test]
    fn every_tag_type_round_trips() {
        let all = Tag::Compound(vec![
            ("b".into(), Tag::Byte(-3)),
            ("s".into(), Tag::Short(-300)),
            ("i".into(), Tag::Int(-70000)),
            ("l".into(), Tag::Long(-5_000_000_000)),
            ("f".into(), Tag::Float(1.5)),
            ("d".into(), Tag::Double(-2.25)),
            ("ba".into(), Tag::ByteArray(vec![1, 2, 3])),
            ("str".into(), Tag::String("ünïcode".into())),
            ("empty".into(), Tag::List(STRING, vec![])),
            ("ia".into(), Tag::IntArray(vec![1, -2])),
            ("la".into(), Tag::LongArray(vec![1, -2])),
            ("nested".into(), Tag::Compound(vec![("x".into(), Tag::Byte(1))])),
        ]);
        let bytes = write("root", &all);
        assert_eq!(read(&bytes).unwrap(), ("root".to_string(), all));
    }

    #[test]
    fn a_truncated_or_bogus_file_is_an_error_not_a_panic() {
        let bytes = write("", &sample());
        for cut in 1..bytes.len() {
            let _ = read(&bytes[..cut]);
        }
        assert!(read(&bytes[..bytes.len() - 4]).is_err());
        assert!(read(b"").is_err());
        assert!(read(b"\x08\x00\x00").is_err(), "a root that is not a compound");
        // A length field claiming far more data than the file holds.
        assert!(read(b"\x0a\x00\x00\x07\x00\x01x\x7f\xff\xff\xff").is_err());
    }

    #[test]
    fn a_missing_list_is_created_on_demand() {
        let mut root = Tag::Compound(vec![]);
        root.list_mut("servers", COMPOUND).unwrap().push(Tag::Byte(1));
        assert_eq!(root.get("servers"), Some(&Tag::List(COMPOUND, vec![Tag::Byte(1)])));
    }
}
