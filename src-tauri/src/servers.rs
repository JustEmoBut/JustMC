//! The multiplayer server list of an instance, and what those servers say
//! when asked.
//!
//! Two halves that only meet in the UI:
//!
//! - `.minecraft/servers.dat` is the game's own list, plain NBT. Reading it
//!   means the launcher shows the servers a player already has; writing it
//!   means one can be added without starting the game first. The file is
//!   round-tripped through `nbt`, so keys this launcher does not understand
//!   survive an edit.
//! - `ping` speaks the server-list protocol over TCP: a handshake, a status
//!   request, and a JSON reply with the MOTD, the player count and an icon.
//!   It is the same exchange the game's own multiplayer screen makes.
//!
//! Never touched while the game is running: the client rewrites `servers.dat`
//! wholesale on exit and would drop anything added underneath it, so the UI
//! disables editing exactly as it does for worlds.

use crate::error::{Error, Result};
use crate::instance;
use crate::nbt::{self, Tag};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FILE: &str = "servers.dat";
/// The default port, and the one left off an address that uses it.
const PORT: u16 = 25565;
/// How long a whole ping may take. A server that is down looks exactly like
/// one that is slow, so this is the wait before "offline" is shown.
const TIMEOUT: Duration = Duration::from_secs(5);
/// The protocol version sent in the handshake. Status is answered whatever
/// this says — it is only echoed back in the reply's version block — so the
/// launcher does not have to track the instance's version to ask.
const PROTOCOL: i32 = 767;
/// The most a status reply may be. Real ones are a few kilobytes, mostly
/// favicon; the cap is what stops a hostile address from being a memory bomb.
const MAX_REPLY: usize = 2 * 1024 * 1024;

/// One entry of the server list, as the UI shows it.
#[derive(Serialize)]
pub struct Server {
    /// Position in the file, and the handle every command takes: the list has
    /// no ids and two entries may share a name and an address.
    pub index: usize,
    pub name: String,
    /// Exactly as the file stores it, port and all — that is what gets pinged.
    pub ip: String,
    /// The icon the game cached the last time it connected, as a data URI.
    pub icon: Option<String>,
}

/// What a server answered. Only what the list needs, out of a reply that also
/// carries mod lists and sample player names.
#[derive(Serialize)]
pub struct Status {
    /// The version the server reports, e.g. "Paper 1.21.1".
    pub version: String,
    pub online: u32,
    pub max: u32,
    /// The MOTD, flattened to plain text: the reply's own formatting is chat
    /// components and colour codes, which this list does not render.
    pub motd: String,
    /// A `data:image/png;base64,…` URI when the server publishes one.
    pub favicon: Option<String>,
    /// Round trip in milliseconds.
    pub latency_ms: u64,
}

/// The status reply's JSON. Every field is optional because proxies and
/// modded servers do leave them out.
#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    version: ReplyVersion,
    #[serde(default)]
    players: ReplyPlayers,
    #[serde(default)]
    description: serde_json::Value,
    #[serde(default)]
    favicon: Option<String>,
}

#[derive(Deserialize, Default)]
struct ReplyVersion {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct ReplyPlayers {
    #[serde(default)]
    online: u32,
    #[serde(default)]
    max: u32,
}

async fn file(id: &str) -> Result<PathBuf> {
    Ok(instance::get(id).await?.game_dir().join(FILE))
}

/// Read the file into a tag tree. A missing one is an empty list, not an
/// error: an instance that has never been played has no `servers.dat`.
async fn load(path: &PathBuf) -> Result<Tag> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(nbt::read(&bytes)?.1),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Tag::Compound(Vec::new())),
        Err(e) => Err(e.into()),
    }
}

async fn save(path: &PathBuf, root: &Tag) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Written beside the real file and renamed over it: a half-written
    // servers.dat is a list the player loses.
    let tmp = path.with_extension("dat.part");
    tokio::fs::write(&tmp, nbt::write("", root)).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

fn entries(root: &Tag) -> &[Tag] {
    match root.get("servers") {
        Some(Tag::List(_, items)) => items,
        _ => &[],
    }
}

/// The server list of an instance, in the order the game shows it.
pub async fn list(id: &str) -> Result<Vec<Server>> {
    let root = load(&file(id).await?).await?;
    Ok(entries(&root)
        .iter()
        .enumerate()
        .map(|(index, entry)| Server {
            index,
            name: entry.get("name").and_then(Tag::as_str).unwrap_or_default().to_string(),
            ip: entry.get("ip").and_then(Tag::as_str).unwrap_or_default().to_string(),
            icon: entry
                .get("icon")
                .and_then(Tag::as_str)
                .filter(|icon| !icon.is_empty())
                .map(|icon| format!("data:image/png;base64,{icon}")),
        })
        .collect())
}

/// Append a server to the list. The game accepts duplicates and so does this;
/// what it does not accept is an empty address.
pub async fn add(id: &str, name: &str, ip: &str) -> Result<()> {
    let (name, ip) = (name.trim(), ip.trim());
    if ip.is_empty() {
        return Err(Error::msg("A server needs an address."));
    }
    let path = file(id).await?;
    let mut root = load(&path).await?;
    let list = root
        .list_mut("servers", 10)
        .ok_or_else(|| Error::msg("servers.dat holds something other than a server list."))?;
    list.push(Tag::Compound(vec![
        ("name".to_string(), Tag::String(if name.is_empty() { ip.into() } else { name.into() })),
        ("ip".to_string(), Tag::String(ip.to_string())),
    ]));
    save(&path, &root).await
}

/// Remove the entry at `index`. Positions come from the same read the UI
/// rendered, so a stale one is refused rather than deleting a neighbour.
pub async fn remove(id: &str, index: usize, ip: &str) -> Result<()> {
    let path = file(id).await?;
    let mut root = load(&path).await?;
    let list = root
        .list_mut("servers", 10)
        .ok_or_else(|| Error::msg("servers.dat holds something other than a server list."))?;
    let matches = list
        .get(index)
        .and_then(|entry| entry.get("ip"))
        .and_then(Tag::as_str)
        .is_some_and(|found| found == ip);
    if !matches {
        return Err(Error::msg("The server list changed. Reopen it and try again."));
    }
    list.remove(index);
    save(&path, &root).await
}

// ------------------------------------------------------------------ pinging

/// Split `host:port`, defaulting the port. IPv6 in brackets is handled because
/// `servers.dat` stores whatever the player typed.
fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(PORT);
            return (host.to_string(), port);
        }
    }
    match address.rsplit_once(':') {
        // A bare IPv6 address has colons but no port.
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host.to_string(), port),
            Err(_) => (address.to_string(), PORT),
        },
        _ => (address.to_string(), PORT),
    }
}

/// A varint, the protocol's length and id encoding.
fn varint(value: i32) -> Vec<u8> {
    let mut value = value as u32;
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// Read one varint off the wire. Five bytes is the format's maximum; anything
/// longer is a stream that is not speaking this protocol.
async fn read_varint<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<i32> {
    let mut value: u32 = 0;
    for shift in 0..5 {
        let byte = stream.read_u8().await?;
        value |= ((byte & 0x7f) as u32) << (shift * 7);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(Error::msg("That is not a Minecraft server."))
}

/// A packet: its length, then its id, then its body.
fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = varint(id);
    inner.extend_from_slice(body);
    let mut out = varint(inner.len() as i32);
    out.extend_from_slice(&inner);
    out
}

/// Flatten a chat component to plain text. The MOTD is a string on old
/// servers, an object with `text` and `extra` on new ones, and the list wants
/// neither markup nor colour codes.
fn plain_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items.iter().map(plain_text).collect(),
        serde_json::Value::Object(map) => {
            let mut out = map.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
            if let Some(extra) = map.get("extra") {
                out.push_str(&plain_text(extra));
            }
            out
        }
        _ => String::new(),
    }
}

/// Strip the section-sign colour codes servers put in a MOTD.
fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Ask a server for its status.
///
/// ponytail: no SRV lookup, so a server published only as `_minecraft._tcp`
/// with no A record on the bare host cannot be reached here. Doing it without
/// a resolver crate means writing DNS packets *and* finding the system's
/// resolver on every platform; add `hickory-resolver` when it matters.
pub async fn ping(address: String) -> Result<Status> {
    let (host, port) = split_address(&address);
    if host.is_empty() {
        return Err(Error::msg("That server has no address."));
    }
    tokio::time::timeout(TIMEOUT, status(host, port))
        .await
        .map_err(|_| Error::msg("The server did not answer in time."))?
}

async fn status(host: String, port: u16) -> Result<Status> {
    let started = Instant::now();
    let mut stream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
    // Every status ping is one short exchange; Nagle would sit on the
    // handshake waiting for more to send.
    let _ = stream.set_nodelay(true);

    let mut handshake = varint(PROTOCOL);
    handshake.extend_from_slice(&varint(host.len() as i32));
    handshake.extend_from_slice(host.as_bytes());
    handshake.extend_from_slice(&port.to_be_bytes());
    handshake.extend_from_slice(&varint(1)); // next state: status
    stream.write_all(&packet(0x00, &handshake)).await?;
    stream.write_all(&packet(0x00, &[])).await?; // status request
    stream.flush().await?;

    let length = read_varint(&mut stream).await?;
    if read_varint(&mut stream).await? != 0x00 {
        return Err(Error::msg("The server answered with something else."));
    }
    let text_len = read_varint(&mut stream).await?;
    if text_len <= 0 || text_len as usize > MAX_REPLY || text_len > length.max(0) {
        return Err(Error::msg("The server sent a status reply that makes no sense."));
    }
    let mut text = vec![0u8; text_len as usize];
    stream.read_exact(&mut text).await?;

    // ponytail: the round trip stands in for the protocol's own ping packet.
    // One exchange less, and the number a player reads is the same order.
    let latency_ms = started.elapsed().as_millis() as u64;
    let reply: Reply = serde_json::from_slice(&text)
        .map_err(|_| Error::msg("The server's status reply was not readable."))?;

    Ok(Status {
        version: reply.version.name,
        online: reply.players.online,
        max: reply.players.max,
        motd: strip_codes(&plain_text(&reply.description)),
        // A favicon is already a data URI in the reply; anything else is not
        // one this app will put in an <img>.
        favicon: reply.favicon.filter(|f| f.starts_with("data:image/")),
        latency_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_keeps_its_port_or_gets_the_default() {
        assert_eq!(split_address("mc.hypixel.net"), ("mc.hypixel.net".into(), 25565));
        assert_eq!(split_address(" play.example.com:25566 "), ("play.example.com".into(), 25566));
        assert_eq!(split_address("[::1]:25570"), ("::1".into(), 25570));
        assert_eq!(split_address("[::1]"), ("::1".into(), 25565));
        // A bare IPv6 address is all colons and no port.
        assert_eq!(split_address("fe80::1"), ("fe80::1".into(), 25565));
        // A port that is not a number is part of the host, not a panic.
        assert_eq!(split_address("example.com:lots"), ("example.com:lots".into(), 25565));
    }

    #[test]
    fn varints_match_the_protocol() {
        assert_eq!(varint(0), vec![0x00]);
        assert_eq!(varint(1), vec![0x01]);
        assert_eq!(varint(127), vec![0x7f]);
        assert_eq!(varint(128), vec![0x80, 0x01]);
        assert_eq!(varint(767), vec![0xff, 0x05]);
        assert_eq!(varint(-1), vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    #[tokio::test]
    async fn a_varint_round_trips_off_the_wire() {
        for value in [0, 1, 127, 128, 2097151, i32::MAX, -1] {
            let bytes = varint(value);
            assert_eq!(read_varint(&mut &bytes[..]).await.unwrap(), value, "{value}");
        }
        // Five continuation bytes is past the format's limit.
        assert!(read_varint(&mut &[0x80u8, 0x80, 0x80, 0x80, 0x80][..]).await.is_err());
    }

    #[test]
    fn a_motd_flattens_whatever_shape_it_arrives_in() {
        let plain = serde_json::json!("A §aMinecraft §rServer");
        assert_eq!(strip_codes(&plain_text(&plain)), "A Minecraft Server");

        let component = serde_json::json!({
            "text": "Welcome ",
            "extra": [{ "text": "back", "color": "green" }, { "text": "!" }],
        });
        assert_eq!(strip_codes(&plain_text(&component)), "Welcome back!");

        // Nothing readable in it is an empty MOTD, not an error.
        assert_eq!(plain_text(&serde_json::json!(null)), "");
        assert_eq!(plain_text(&serde_json::json!({ "translate": "multiplayer.status" })), "");
    }

    #[test]
    fn a_reply_missing_everything_optional_still_parses() {
        let reply: Reply = serde_json::from_str("{}").unwrap();
        assert_eq!(reply.players.online, 0);
        assert!(reply.version.name.is_empty());
        assert!(reply.favicon.is_none());
    }
}
