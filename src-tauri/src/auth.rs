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
/// The current first-party endpoint: what the official launcher uses.
const MC_LAUNCHER_LOGIN_URL: &str = "https://api.minecraftservices.com/launcher/login";
/// The older one, kept as a fallback — see `minecraft_token`.
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
    /// Minecraft services token. Empty for offline accounts, and empty in
    /// `accounts.json` whenever the OS keychain took it — see `secrets`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access_token: String,
    /// Microsoft refresh token, used to renew without re-login. Stored beside
    /// the access token, wherever that ends up.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub refresh_token: String,
    /// Unix seconds after which `access_token` must be refreshed.
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub xuid: String,
    /// The skin being worn, which the avatar is cut from. Empty for offline
    /// accounts and until the profile is next read.
    #[serde(default)]
    pub skin_url: String,
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
            other => return Err(Error::msg(device_code_error(other, &body))),
        }
    }
}

/// Microsoft answers a failed device-code poll with a machine-readable `error`
/// and a paragraph of English in `error_description`. The codes are the errors
/// real users hit — a declined consent screen or a code left too long — so each
/// gets a sentence that says what to do, the way XSTS's `XErr` codes do.
fn device_code_error(code: &str, body: &serde_json::Value) -> String {
    match code {
        "authorization_declined" => {
            "Sign-in was declined in the browser. Try again and choose Yes.".into()
        }
        "expired_token" => {
            "The sign-in code expired before it was used. Try again.".into()
        }
        "bad_verification_code" => {
            "Microsoft did not recognise the sign-in code. Try again.".into()
        }
        other => {
            // Anything unlisted: Microsoft's own description beats its code.
            let detail = body["error_description"]
                .as_str()
                .map(|d| d.lines().next().unwrap_or(d).trim())
                .filter(|d| !d.is_empty())
                .unwrap_or(if other.is_empty() { "unknown error" } else { other });
            format!("Sign-in failed: {detail}")
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

    let mc = minecraft_token(&client, &format!("XBL3.0 x={};{}", xsts.uhs()?, xsts.token)).await?;

    let profile_resp = client
        .get(MC_PROFILE_URL)
        .bearer_auth(&mc.access_token)
        .send()
        .await?;
    if profile_resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::msg("This account does not own Minecraft: Java Edition."));
    }
    let profile: crate::skins::Profile = profile_resp.error_for_status()?.json().await?;

    // Renew a minute early so a launch never starts with a token about to die.
    let lifetime = mc.expires_in.min(ms.expires_in).saturating_sub(60);
    Ok(Account {
        id: dashed_uuid(&profile.id),
        name: profile.name.clone(),
        kind: AccountKind::Microsoft,
        access_token: mc.access_token,
        refresh_token: ms.refresh_token,
        expires_at: now() + lifetime,
        xuid: xsts.xuid(),
        skin_url: profile.skin_url(),
    })
}

#[derive(Deserialize)]
struct McToken {
    access_token: String,
    expires_in: u64,
}

/// Trade an XSTS token for a Minecraft one.
///
/// Two endpoints answer this, with the same fields in the reply. `launcher/
/// login` is the current first-party flow — what the official launcher calls,
/// with a `PC_LAUNCHER` platform — and `authentication/login_with_xbox` is the
/// older one this launcher used, which will not be around forever.
///
/// The new one is tried first and the old one catches a failure, because
/// there is no test account here to prove the new shape against a live reply:
/// if it is wrong, or Mojang gates it on something an unofficial client does
/// not have, users still sign in. Drop the fallback once the new path has been
/// seen working.
async fn minecraft_token(client: &reqwest::Client, xtoken: &str) -> Result<McToken> {
    let launcher = client
        .post(MC_LAUNCHER_LOGIN_URL)
        .json(&json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }))
        .send()
        .await;

    if let Ok(response) = launcher {
        if response.status().is_success() {
            if let Ok(token) = response.json::<McToken>().await {
                return Ok(token);
            }
        }
    }

    Ok(client
        .post(MC_LOGIN_URL)
        .json(&json!({ "identityToken": xtoken }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
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
        skin_url: String::new(),
    })
}

// ------------------------------------------------------------------- storage
//
// A live refresh token is a login. `accounts.json` holds who the accounts are;
// the OS keychain holds what proves it — Credential Manager on Windows,
// Keychain on macOS, the Secret Service on Linux.
//
// The keychain is not assumed to be there. A portable stick, a headless Linux
// box with no session bus, a locked keyring: any of those fail, and the tokens
// go back in the file rather than the user being logged out. That fallback is
// the same weakness as before, but now it is the exception rather than the
// only path.

/// The keychain service name every entry lives under.
const SERVICE: &str = "JustLauncher";

/// The secret half of an account, as one keychain entry.
#[derive(Serialize, Deserialize, Default)]
struct Secrets {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

impl Secrets {
    fn is_empty(&self) -> bool {
        self.access_token.is_empty() && self.refresh_token.is_empty()
    }
}

/// Keychain calls are blocking, and on Linux they are a D-Bus round trip, so
/// they never run on the async runtime's thread directly.
async fn on_keychain<T, F>(work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> keyring::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work).await.ok()?.ok()
}

/// Read one account's secrets back. A missing entry is `None`, which is also
/// what an unavailable keychain looks like — both mean "the file is all there
/// is".
async fn read_secrets(id: &str) -> Option<Secrets> {
    let id = id.to_string();
    let json = on_keychain(move || keyring::Entry::new(SERVICE, &id)?.get_password()).await?;
    serde_json::from_str(&json).ok()
}

/// Store one account's secrets, reporting whether the keychain took them.
async fn write_secrets(id: &str, secrets: &Secrets) -> bool {
    let id = id.to_string();
    let Ok(json) = serde_json::to_string(secrets) else { return false };
    on_keychain(move || keyring::Entry::new(SERVICE, &id)?.set_password(&json)).await.is_some()
}

/// Drop an account's secrets. Called when the account goes, so a removed login
/// does not linger in the keychain.
pub async fn forget(id: &str) {
    let id = id.to_string();
    on_keychain(move || keyring::Entry::new(SERVICE, &id)?.delete_credential()).await;
}

pub async fn load_all() -> Vec<Account> {
    let mut accounts: Vec<Account> = tokio::fs::read_to_string(paths::accounts_file())
        .await
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();

    // A token still in the file is either a pre-keychain install or one where
    // the keychain refused; either way the file wins, because it is the copy
    // that was written last.
    let mut plaintext = false;
    for account in &mut accounts {
        if !account.refresh_token.is_empty() || !account.access_token.is_empty() {
            plaintext = true;
            continue;
        }
        if let Some(secrets) = read_secrets(&account.id).await {
            account.access_token = secrets.access_token;
            account.refresh_token = secrets.refresh_token;
        }
    }

    // Migrate on the way past: an old file gets rewritten without its tokens
    // the first time anything reads it.
    if plaintext {
        let _ = save_all(&accounts).await;
    }
    accounts
}

pub async fn save_all(accounts: &[Account]) -> Result<()> {
    let mut stored = accounts.to_vec();
    for account in &mut stored {
        let secrets = Secrets {
            access_token: std::mem::take(&mut account.access_token),
            refresh_token: std::mem::take(&mut account.refresh_token),
        };
        if secrets.is_empty() {
            continue; // an offline account has nothing to hide
        }
        if !write_secrets(&account.id, &secrets).await {
            // No keychain: put them back in the file rather than losing the
            // login. The user stays signed in; the tokens are as exposed as
            // they were before this existed.
            account.access_token = secrets.access_token;
            account.refresh_token = secrets.refresh_token;
        }
    }

    let path = paths::accounts_file();
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(&path, serde_json::to_vec_pretty(&stored)?).await?;
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
    fn a_saved_account_carries_no_tokens_in_its_json() {
        // What `save_all` writes for an account whose secrets the keychain
        // took: identity, no credentials.
        let mut account = offline_account("Notch").unwrap();
        account.kind = AccountKind::Microsoft;
        let json = serde_json::to_string(&account).unwrap();
        assert!(!json.contains("access_token"), "{json}");
        assert!(!json.contains("refresh_token"), "{json}");
        assert!(json.contains("Notch"));

        // And one the keychain refused still round-trips, so a fallback file
        // keeps the user logged in.
        account.refresh_token = "secret".into();
        let json = serde_json::to_string(&account).unwrap();
        let back: Account = serde_json::from_str(&json).unwrap();
        assert_eq!(back.refresh_token, "secret");
        assert!(back.access_token.is_empty());
    }

    #[test]
    fn undashed_uuids_get_dashes() {
        assert_eq!(
            dashed_uuid("069a79f444e94726a5befca90e38aaf5"),
            "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
    }
}
