//! Building the JVM command line and running the game.

use crate::auth::{Account, AccountKind};
use crate::error::{Error, Result};
use crate::install;
use crate::instance::{self, Instance};
use crate::java;
use crate::jre;
use crate::mojang::{Arg, VersionJson, rules_allow};
use crate::paths;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Instances with a game process attached, each mapped to the channel that
/// asks it to stop. Two copies of Minecraft sharing one `.minecraft` corrupt
/// saves, so a second launch is refused until the first exits.
///
/// The sender is `None` between claiming the slot and spawning the process:
/// installing can take minutes and the instance is already claimed then.
static RUNNING: LazyLock<Mutex<HashMap<String, Option<Stopper>>>> = LazyLock::new(Mutex::default);

type Stopper = tokio::sync::oneshot::Sender<()>;

/// Removes the instance from `RUNNING` however `launch` ends -- an early error
/// must not leave the instance permanently unlaunchable.
struct RunningGuard(String);

impl RunningGuard {
    /// `None` if this instance already has a process.
    fn claim(id: &str) -> Option<Self> {
        let mut running = RUNNING.lock().unwrap();
        if running.contains_key(id) {
            return None;
        }
        running.insert(id.to_string(), None);
        Some(Self(id.to_string()))
    }

    /// Publish the channel that stops this instance's process.
    fn armed(&self, stopper: Stopper) {
        RUNNING.lock().unwrap().insert(self.0.clone(), Some(stopper));
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.lock().unwrap().remove(&self.0);
    }
}

/// Ask a running instance's game process to stop. Returns whether one was
/// listening; a game that ignores the request is killed outright.
pub fn stop(id: &str) -> bool {
    let stopper = RUNNING.lock().unwrap().get_mut(id).and_then(Option::take);
    stopper.is_some_and(|tx| tx.send(()).is_ok())
}

#[derive(Serialize, Clone)]
pub struct LogLine {
    pub instance: String,
    pub line: String,
}

#[derive(Serialize, Clone)]
pub struct Exited {
    pub instance: String,
    pub code: i32,
}

/// Expand `${placeholder}` tokens using the substitution table. Unknown tokens
/// are left as-is: Mojang adds new ones over time and a literal is less harmful
/// than dropping the argument.
fn substitute(arg: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push_str("${");
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn expand(args: &[Arg], vars: &HashMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Arg::Plain(s) => out.push(substitute(s, vars)),
            Arg::Conditional { rules, value } => {
                if rules_allow(rules) {
                    out.extend(value.clone().into_vec().iter().map(|s| substitute(s, vars)));
                }
            }
        }
    }
    out
}

fn classpath_string(jars: &[std::path::PathBuf]) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    jars.iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep)
}

/// The full argument list, JVM args first, then the main class, then game args.
pub fn build_command(
    version: &VersionJson,
    instance: &Instance,
    account: &Account,
    jars: &[std::path::PathBuf],
    natives: &Path,
) -> Result<Vec<String>> {
    if version.main_class.is_empty() {
        return Err(Error::msg("Version metadata has no main class."));
    }
    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("natives_directory", natives.to_string_lossy().into_owned());
    vars.insert("launcher_name", "JustLauncher".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath", classpath_string(jars));
    vars.insert("classpath_separator", if cfg!(windows) { ";".into() } else { ":".into() });
    vars.insert("library_directory", paths::libraries().to_string_lossy().into_owned());
    vars.insert("version_name", instance.version_id());
    vars.insert("game_directory", instance.game_dir().to_string_lossy().into_owned());
    vars.insert("assets_root", paths::assets().to_string_lossy().into_owned());
    let assets_index_name = version
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| "legacy".into());
    // Only legacy versions read ${game_assets}, and for those the install step
    // has materialised the named tree under this exact path.
    vars.insert(
        "game_assets",
        paths::assets()
            .join("virtual")
            .join(&assets_index_name)
            .to_string_lossy()
            .into_owned(),
    );
    vars.insert("assets_index_name", assets_index_name);
    vars.insert("auth_player_name", account.name.clone());
    vars.insert("auth_uuid", account.id.clone());
    vars.insert("auth_xuid", account.xuid.clone());
    vars.insert("clientid", String::new());
    // Offline accounts have no token; the game accepts a placeholder and simply
    // fails to reach online servers, which is the expected offline behaviour.
    vars.insert(
        "auth_access_token",
        if account.access_token.is_empty() { "0".into() } else { account.access_token.clone() },
    );
    vars.insert("auth_session", format!("token:{}", vars["auth_access_token"]));
    vars.insert(
        "user_type",
        match account.kind {
            AccountKind::Microsoft => "msa".into(),
            AccountKind::Offline => "legacy".into(),
        },
    );
    vars.insert("version_type", version.kind.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert("resolution_width", "854".into());
    vars.insert("resolution_height", "480".into());

    let mut cmd = vec![
        format!("-Xmx{}M", instance.memory_mb),
        format!("-Xms{}M", (instance.memory_mb / 2).max(512)),
    ];
    cmd.extend(
        instance
            .jvm_args
            .split_whitespace()
            .map(|s| substitute(s, &vars)),
    );

    match &version.arguments {
        Some(args) => cmd.extend(expand(&args.jvm, &vars)),
        // Pre-1.13 metadata carries no JVM arguments; supply the two the
        // launcher is expected to provide.
        None => {
            cmd.push(format!("-Djava.library.path={}", natives.display()));
            cmd.push("-cp".into());
            cmd.push(classpath_string(jars));
        }
    }

    cmd.push(version.main_class.clone());

    match (&version.arguments, &version.minecraft_arguments) {
        (Some(args), _) => cmd.extend(expand(&args.game, &vars)),
        (None, Some(legacy)) => {
            cmd.extend(legacy.split_whitespace().map(|s| substitute(s, &vars)))
        }
        (None, None) => return Err(Error::msg("Version metadata has no game arguments.")),
    }

    Ok(cmd)
}

/// Forward one output stream of the game process to the UI, line by line.
async fn pump_log<R>(
    reader: Option<R>,
    app: AppHandle,
    id: String,
    file: Arc<tokio::sync::Mutex<tokio::fs::File>>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(reader) = reader else { return };
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        // Unredacted: this file stays on the user's disk. Anything leaving the
        // app goes through redact.ts instead.
        let mut file = file.lock().await;
        let _ = file.write_all(line.as_bytes()).await;
        let _ = file.write_all(b"
").await;
        drop(file);
        let _ = app.emit("game-log", LogLine { instance: id.clone(), line });
    }
}

/// Install if needed, then spawn the game and stream its output to the UI.
pub async fn launch(app: &AppHandle, mut instance: Instance, account: Account) -> Result<()> {
    let _guard = RunningGuard::claim(&instance.id).ok_or_else(|| {
        Error::msg(format!("{} is already running.", instance.name))
    })?;
    let version = install::install(app, &mut instance).await?;
    let cp = install::classpath(&version, &instance.mc_version)?;

    let required = version.java_version.as_ref().map_or(8, |j| j.major_version);
    if let Some(total) = java::physical_memory_mb() {
        if instance.memory_mb > total {
            return Err(Error::msg(format!(
                "This instance asks for {} MB of memory, but the machine has {total} MB. Lower the memory setting.",
                instance.memory_mb
            )));
        }
    }
    let java_bin = match java::find(Some(instance.java_path.as_str()), required) {
        Some(path) => {
            // An override is taken as given, so a mismatch has to be caught
            // here: 1.8.9 on a modern JVM crashes in a way that looks like a
            // broken install rather than a settings mistake.
            if let Some(java) = java::inspect(&path) {
                let have = java.major;
                if !instance.java_path.is_empty() && !java::is_compatible(have, required) {
                    return Err(Error::msg(format!(
                        "Minecraft {} needs Java {required}, but this instance is set to use Java {have}. Change the Java setting, or clear it to let the launcher pick.",
                        instance.mc_version
                    )));
                }
                // A 32-bit JVM refuses to start at all with a large -Xmx, and
                // the error it prints says nothing about the real cause.
                if !java.bit64 && instance.memory_mb > 2048 {
                    return Err(Error::msg(format!(
                        "This Java is 32-bit and cannot use {} MB of memory. Lower the memory setting to 2048 MB or less, or install a 64-bit Java.",
                        instance.memory_mb
                    )));
                }
            }
            path
        }
        // Nothing suitable on this machine: fetch the runtime Mojang ships for
        // this version rather than telling the user to go install a JDK.
        None => {
            let component = version.java_version.as_ref().map(|j| j.component.as_str());
            jre::ensure(app, component, required).await?
        }
    };
    let args = build_command(&version, &instance, &account, &cp.jars, &instance.natives_dir())?;

    tokio::fs::create_dir_all(instance.game_dir()).await?;

    let mut command = tokio::process::Command::new(&java_bin);
    command
        .args(&args)
        .current_dir(instance.game_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn().map_err(|e| {
        Error::msg(format!("Failed to start Java at {java_bin}: {e}"))
    })?;

    instance.last_played = instance::now_secs();
    instance.save().await?;
    let started = instance.last_played;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let id = instance.id.clone();

    // Truncated per launch, like the game's own latest.log.
    let log_dir = instance.dir().join("logs");
    tokio::fs::create_dir_all(&log_dir).await?;
    let log_file = Arc::new(tokio::sync::Mutex::new(
        tokio::fs::File::create(log_dir.join("latest.log")).await?,
    ));

    // The game outlives this command; detach it and report through events so a
    // crash after five minutes still reaches the log view.
    let app_handle = app.clone();
    let guard = _guard;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    guard.armed(stop_tx);
    tokio::spawn(async move {
        let _guard = guard; // released when the process exits, not when launch returns
        let pumps = async {
            tokio::join!(
                pump_log(stdout, app_handle.clone(), id.clone(), log_file.clone()),
                pump_log(stderr, app_handle.clone(), id.clone(), log_file.clone()),
            );
        };
        tokio::select! {
            _ = pumps => {}
            // Stop requested: the pipes are abandoned mid-stream, which is the
            // point -- the user wants the process gone, not its last lines.
            _ = stop_rx => {
                let _ = child.start_kill();
            }
        }
        let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);

        // Re-read rather than reusing the copy captured at launch: the settings
        // dialog may have written the file while the game was running.
        if let Ok(mut played) = instance::get(&id).await {
            played.play_time += instance::now_secs().saturating_sub(started);
            let _ = played.save().await;
        }
        let _ = app_handle.emit("game-exited", Exited { instance: id, code });
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RunningGuard;

    #[test]
    fn second_launch_is_refused_until_the_first_ends() {
        let first = RunningGuard::claim("dup").expect("first claim");
        assert!(RunningGuard::claim("dup").is_none());
        assert!(RunningGuard::claim("other").is_some());
        drop(first);
        assert!(RunningGuard::claim("dup").is_some());
    }

    use super::*;

    fn vars() -> HashMap<&'static str, String> {
        let mut v = HashMap::new();
        v.insert("auth_player_name", "Notch".to_string());
        v.insert("version_name", "1.21".to_string());
        v
    }

    #[test]
    fn substitutes_known_tokens() {
        assert_eq!(substitute("${auth_player_name}", &vars()), "Notch");
        assert_eq!(substitute("--v=${version_name}!", &vars()), "--v=1.21!");
        assert_eq!(substitute("plain", &vars()), "plain");
    }

    #[test]
    fn leaves_unknown_and_malformed_tokens_intact() {
        assert_eq!(substitute("${nope}", &vars()), "${nope}");
        assert_eq!(substitute("${unterminated", &vars()), "${unterminated");
        assert_eq!(substitute("a${nope}b${version_name}", &vars()), "a${nope}b1.21");
    }
}
