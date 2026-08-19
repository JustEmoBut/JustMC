//! Microsoft account login via the OAuth 2.0 device code flow.
//!
//! Device code is used rather than an auth-code redirect because it needs no
//! embedded browser, no loopback HTTP server, and no client secret — the user
//! opens a URL, types a code, and the launcher polls for the token.
//!
//! The chain is: Microsoft token -> Xbox Live token -> XSTS token ->
//! Minecraft token -> profile.

use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::json;

const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

/// Azure application (client) ID. There is no usable default: every launcher
/// must register its own Azure app and have it approved by Mojang, so this is
/// supplied at build time or via the environment rather than hardcoded.
fn client_id() -> Result<String> {
    if let Ok(id) = std::env::var("JUSTLAUNCHER_MSA_CLIENT_ID") {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    match option_env!("JUSTLAUNCHER_MSA_CLIENT_ID") {
        Some(id) if !id.is_empty() => Ok(id.to_string()),
        _ => Err(Error::msg(
            "No Microsoft client ID configured. Register an Azure application \
             with the 'XboxLive.signin' scope and set JUSTLAUNCHER_MSA_CLIENT_ID. \
             Until then you can use an offline account.",
        )),
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub kind: AccountKind,
    /// Minecraft services token. Empty for offline accounts.
    #[serde(default)]
    pub access_token: String,
    /// Microsoft refresh token, used to renew without re-login.
    #[serde(default)]
    pub refresh_token: String,
    /// Unix seconds after which `access_token` must be refreshed.
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub xuid: String,
}

impl Account {
    pub fn is_expired(&self) -> bool {
        self.kind == AccountKind::Microsoft && now() >= self.expires_at
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// -------------------------------------------------------------- device code

#[derive(Deserialize, Serialize, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub async fn start_device_code() -> Result<DeviceCode> {
    let client = reqwest::Client::new();
    let resp = client
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", client_id()?.as_str()), ("scope", SCOPE)])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::msg(format!("device code request failed: {}", resp.text().await?)));
    }
    Ok(resp.json().await?)
}

#[derive(Deserialize)]
struct MsToken {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

/// Poll until the user finishes signing in, then run the full Xbox chain.
/// Returns an error if the code expires or is declined.
pub async fn poll_device_code(code: DeviceCode) -> Result<Account> {
    let client = reqwest::Client::new();
    let id = client_id()?;
    let deadline = now() + code.expires_in;
    // Microsoft raises `interval` on slow_down; keep our own copy to bump.
    let mut interval = code.interval.max(1);

    loop {
        if now() >= deadline {
            return Err(Error::msg("Sign-in timed out. Please try again."));
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let resp = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", id.as_str()),
                ("device_code", code.device_code.as_str()),
            ])
            .send()
            .await?;

        if resp.status().is_success() {
            let token: MsToken = resp.json().await?;
            return finish_login(token).await;
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        match body["error"].as_str().unwrap_or("") {
            "authorization_pending" => continue,
            "slow_down" => interval += 5,
            other => {
                return Err(Error::msg(format!(
                    "Sign-in failed: {}",
                    if other.is_empty() { "unknown error" } else { other }
                )))
            }
        }
    }
}

pub async fn refresh(account: &Account) -> Result<Account> {
    if account.refresh_token.is_empty() {
        return Err(Error::msg("Account has no refresh token; please sign in again."));
    }
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id()?.as_str()),
            ("refresh_token", account.refresh_token.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::msg("Session expired; please sign in again."));
    }
    finish_login(resp.json().await?).await
}

// ------------------------------------------------------------- the xbox chain

#[derive(Deserialize)]
struct XboxResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: serde_json::Value,
}

impl XboxResponse {
    /// The user hash, required to build the Minecraft identity token.
    fn uhs(&self) -> Result<String> {
        self.display_claims["xui"][0]["uhs"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::msg("Xbox response is missing the user hash"))
    }

    fn xuid(&self) -> String {
        self.display_claims["xui"][0]["xid"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

async fn finish_login(ms: MsToken) -> Result<Account> {
    let client = reqwest::Client::new();

    let xbl: XboxResponse = client
        .post(XBL_URL)
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms.access_token),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let xsts_resp = client
        .post(XSTS_URL)
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;

    // XSTS uses 401 with an XErr code for the "this account cannot play" cases,
    // which are the ones users actually hit — translate them instead of
    // surfacing a bare HTTP error.
    if !xsts_resp.status().is_success() {
        let body: serde_json::Value = xsts_resp.json().await.unwrap_or_default();
        let msg = match body["XErr"].as_u64().unwrap_or(0) {
            2148916233 => "This Microsoft account has no Xbox profile. Create one at xbox.com first.",
            2148916235 => "Xbox Live is not available in this account's country.",
            2148916236 | 2148916237 => "This account needs adult verification.",
            2148916238 => "This is a child account and must be added to a Family to play.",
            _ => "Xbox authorisation failed.",
        };
        return Err(Error::msg(msg));
    }
    let xsts: XboxResponse = xsts_resp.json().await?;

    #[derive(Deserialize)]
    struct McToken {
        access_token: String,
        expires_in: u64,
    }
    let mc: McToken = client
        .post(MC_LOGIN_URL)
        .json(&json!({
            "identityToken": format!("XBL3.0 x={};{}", xsts.uhs()?, xsts.token),
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    let profile_resp = client
        .get(MC_PROFILE_URL)
        .bearer_auth(&mc.access_token)
        .send()
        .await?;
    if profile_resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::msg("This account does not own Minecraft: Java Edition."));
    }
    let profile: Profile = profile_resp.error_for_status()?.json().await?;

    // Renew a minute early so a launch never starts with a token about to die.
    let lifetime = mc.expires_in.min(ms.expires_in).saturating_sub(60);
    Ok(Account {
        id: dashed_uuid(&profile.id),
        name: profile.name,
        kind: AccountKind::Microsoft,
        access_token: mc.access_token,
        refresh_token: ms.refresh_token,
        expires_at: now() + lifetime,
        xuid: xsts.xuid(),
    })
}

/// Mojang returns UUIDs without dashes; the game wants them dashed.
fn dashed_uuid(raw: &str) -> String {
    match uuid::Uuid::parse_str(raw) {
        Ok(u) => u.hyphenated().to_string(),
        Err(_) => raw.to_string(),
    }
}

// ------------------------------------------------------------------- offline

/// Offline UUIDs must match the server-side derivation (`OfflinePlayer:<name>`
/// hashed as a v3 UUID) or world data keyed by UUID will not carry over.
pub fn offline_account(name: &str) -> Result<Account> {
    let name = name.trim();
    if name.is_empty() || name.len() > 16 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::msg(
            "Username must be 1-16 characters, letters, digits or underscore.",
        ));
    }
    let id = uuid::Uuid::new_v3(
        &uuid::Uuid::NAMESPACE_OID,
        format!("OfflinePlayer:{name}").as_bytes(),
    );
    Ok(Account {
        id: id.hyphenated().to_string(),
        name: name.to_string(),
        kind: AccountKind::Offline,
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: 0,
        xuid: String::new(),
    })
}

// ------------------------------------------------------------------- storage

pub async fn load_all() -> Vec<Account> {
    tokio::fs::read_to_string(paths::accounts_file())
        .await
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub async fn save_all(accounts: &[Account]) -> Result<()> {
    let path = paths::accounts_file();
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(&path, serde_json::to_vec_pretty(accounts)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_stable_and_dashed() {
        let a = offline_account("Notch").unwrap();
        let b = offline_account(" Notch ").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, "Notch");
        assert_eq!(a.id.len(), 36);
    }

    #[test]
    fn offline_names_are_validated() {
        assert!(offline_account("").is_err());
        assert!(offline_account("way_too_long_a_name").is_err());
        assert!(offline_account("bad name!").is_err());
        assert!(offline_account("ok_Name9").is_ok());
    }

    #[test]
    fn undashed_uuids_get_dashes() {
        assert_eq!(
            dashed_uuid("069a79f444e94726a5befca90e38aaf5"),
            "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
    }
}
