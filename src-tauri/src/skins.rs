//! A Microsoft account's skin and cape, through Mojang's profile API.
//!
//! Endpoints and field names as the Minecraft Wiki's "Mojang API" page lists
//! them. No test account exists in this repo, so the calls are unproven
//! against a live reply; every change re-reads the profile rather than
//! trusting what a mutation answers with, which keeps that guess to one shape.

use crate::auth::{Account, AccountKind};
use crate::download;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// What `GET /minecraft/profile` answers, trimmed to what the launcher shows.
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<Texture>,
    #[serde(default)]
    pub capes: Vec<Texture>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Texture {
    pub id: String,
    /// "ACTIVE" for the one being worn.
    #[serde(default)]
    pub state: String,
    pub url: String,
    /// A skin's arm model: "CLASSIC" or "SLIM". Absent on capes.
    #[serde(default)]
    pub variant: String,
    /// A cape's name ("Migrator"). Absent on skins.
    #[serde(default)]
    pub alias: String,
}

impl Texture {
    fn active(&self) -> bool {
        self.state.eq_ignore_ascii_case("active")
    }
}

impl Profile {
    /// The skin being worn, which is what an avatar is cut from.
    pub fn skin_url(&self) -> String {
        self.skins.iter().find(|s| s.active()).map(|s| s.url.clone()).unwrap_or_default()
    }
}

/// The two arm models Mojang accepts.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    Classic,
    Slim,
}

impl Variant {
    fn as_str(self) -> &'static str {
        match self {
            Variant::Classic => "classic",
            Variant::Slim => "slim",
        }
    }
}

fn token(account: &Account) -> Result<&str> {
    if account.kind != AccountKind::Microsoft || account.access_token.is_empty() {
        return Err(Error::msg("Only a Microsoft account has a skin on Mojang's servers."));
    }
    Ok(&account.access_token)
}

pub async fn profile(account: &Account) -> Result<Profile> {
    let client = download::client()?;
    Ok(download::send(client.get(PROFILE_URL).bearer_auth(token(account)?)).await?.json().await?)
}

/// Check the file is a skin before it costs a request: a PNG, 64 wide and
/// 64 or 32 tall (the legacy layout the game still reads).
fn check_png(bytes: &[u8]) -> Result<()> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(Error::msg("A skin has to be a PNG image."));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width != 64 || !(height == 64 || height == 32) {
        return Err(Error::msg(format!("A skin is 64×64 (or 64×32); this image is {width}×{height}.")));
    }
    Ok(())
}

/// A multipart body with a `variant` field and a `file` part, built here
/// rather than through reqwest's `multipart` feature, which would pull in a
/// MIME-guessing crate for one fixed content type.
fn multipart(variant: Variant, png: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("justlauncher-{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(png.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n\
             Content-Type: image/png\r\n\r\n",
            variant.as_str()
        )
        .as_bytes(),
    );
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

pub async fn upload(account: &Account, variant: Variant, png: &[u8]) -> Result<Profile> {
    check_png(png)?;
    let (content_type, body) = multipart(variant, png);
    let request = download::client()?
        .post(format!("{PROFILE_URL}/skins"))
        .bearer_auth(token(account)?)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body);
    download::send(request).await?;
    profile(account).await
}

/// Back to the default skin Mojang assigns.
pub async fn reset(account: &Account) -> Result<Profile> {
    let request = download::client()?.delete(format!("{PROFILE_URL}/skins/active")).bearer_auth(token(account)?);
    download::send(request).await?;
    profile(account).await
}

/// Wear one of the account's capes, or none.
pub async fn set_cape(account: &Account, cape: Option<&str>) -> Result<Profile> {
    let client = download::client()?;
    let url = format!("{PROFILE_URL}/capes/active");
    let request = match cape {
        Some(id) => client.put(url).json(&serde_json::json!({ "capeId": id })),
        None => client.delete(url),
    };
    download::send(request.bearer_auth(token(account)?)).await?;
    profile(account).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn only_a_skin_shaped_png_is_sent() {
        assert!(check_png(&png(64, 64)).is_ok());
        assert!(check_png(&png(64, 32)).is_ok());
        assert!(check_png(&png(128, 128)).is_err());
        assert!(check_png(b"GIF89a not a png at all....").is_err());
    }

    #[test]
    fn the_multipart_body_carries_both_fields() {
        let (content_type, body) = multipart(Variant::Slim, b"PNGDATA");
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.contains("name=\"variant\"\r\n\r\nslim\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\nPNGDATA\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn the_worn_skin_is_the_active_one() {
        let profile: Profile = serde_json::from_str(
            r#"{"id":"x","name":"n","skins":[
                {"id":"a","state":"INACTIVE","url":"https://old","variant":"CLASSIC"},
                {"id":"b","state":"ACTIVE","url":"https://new","variant":"SLIM"}],
               "capes":[{"id":"c","state":"INACTIVE","url":"https://cape","alias":"Migrator"}]}"#,
        )
        .unwrap();
        assert_eq!(profile.skin_url(), "https://new");
        assert_eq!(profile.capes[0].alias, "Migrator");
    }
}
