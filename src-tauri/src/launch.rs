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
use std::time::Duration;
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

/// How long a `pre_launch` or `post_exit` hook may run before it is killed.
const HOOK_TIMEOUT: Duration = Duration::from_secs(300);

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

/// A server address or save folder to drop straight into, from the Servers or
/// Worlds panel. Minecraft calls this Quick Play and has shipped it since 1.20.
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum QuickPlay {
    Multiplayer(String),
    Singleplayer(String),
}

impl QuickPlay {
    fn flag(&self) -> &'static str {
        match self {
            QuickPlay::Multiplayer(_) => "--quickPlayMultiplayer",
            QuickPlay::Singleplayer(_) => "--quickPlaySingleplayer",
        }
    }

    fn value(&self) -> &str {
        match self {
            QuickPlay::Multiplayer(v) | QuickPlay::Singleplayer(v) => v,
        }
    }
}

/// Whether this version understands the Quick Play flags.
///
/// Asked of the metadata rather than of the version number: 1.20+ declares the
/// flags under `is_quick_play_*` feature rules, and a snapshot from the release
/// before it does not — comparing "1.20" against `23w14a` would guess wrong.
/// `rules_allow` never matches a feature rule, so the arguments themselves are
/// dropped and this launcher supplies its own.
///
/// Public because the Servers and Worlds panels ask it before offering to join
/// anything: the version *number* cannot answer it. Minecraft moved to a
/// year-based scheme in 2026, so "is the minor version at least 20" reads 26.2
/// as older than 1.20 and hides a button that works.
pub fn supports_quick_play(version: &VersionJson) -> bool {
    let Some(arguments) = &version.arguments else { return false };
    arguments.game.iter().any(|arg| match arg {
        Arg::Conditional { rules, .. } => rules
            .iter()
            .any(|r| r.features.keys().any(|k| k.starts_with("is_quick_play"))),
        Arg::Plain(_) => false,
    })
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
    quick_play: Option<&QuickPlay>,
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
    // NeoForge's `-DignoreList=${version_name}.jar,...` keeps the vanilla client jar
    // off BootstrapLauncher's module path, and that jar is named after the *Minecraft*
    // version, not the loader profile. Substituting the profile id leaves it on both
    // paths and module resolution fails with a split package.
    vars.insert("version_name", instance.mc_version.clone());
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
    // Mojang's own defaults, and what the placeholders expand to when the
    // instance names no size of its own.
    let (width, height) = match (instance.window_width, instance.window_height) {
        (w, h) if w > 0 && h > 0 => (w, h),
        _ => (854, 480),
    };
    vars.insert("resolution_width", width.to_string());
    vars.insert("resolution_height", height.to_string());

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

    // `--width`/`--height` sit behind a `has_custom_resolution` feature rule
    // that `rules_allow` never matches, so the launcher passes them itself --
    // the same arrangement Quick Play needs. The flags predate the rule and
    // every version this launcher runs understands them.
    if instance.window_width > 0 && instance.window_height > 0 {
        cmd.push("--width".into());
        cmd.push(width.to_string());
        cmd.push("--height".into());
        cmd.push(height.to_string());
    }

    if let Some(quick) = quick_play {
        if !supports_quick_play(version) {
            return Err(Error::msg(format!(
                "Minecraft {} cannot be told what to join from the launcher; that needs 1.20 or newer.",
                instance.mc_version
            )));
        }
        cmd.push(quick.flag().into());
        cmd.push(quick.value().into());
    }

    Ok(cmd)
}

/// Run a user-supplied hook through the system shell and collect its output.
///
/// The shell is the point: a hook is written the way the user would type it in
/// a terminal, quoting and `&&` included, and splitting the string here would
/// only be a worse shell. It runs in the game directory with the instance
/// described in the environment, which is how a script knows what it was
/// called for.
///
/// Never `Err`: a hook that could not start is a failed hook, and the caller
/// decides what that costs.
///
/// A hook that never exits used to wedge the launch with the instance claimed,
/// so one that outlives `HOOK_TIMEOUT` is killed and counts as failed. The
/// budget is generous because a pre-launch hook legitimately syncs files or
/// waits on a server; it is there to catch a hook waiting for input that
/// stdin's `null` will never bring, not to hurry a slow one.
///
/// Public only so `pack_roundtrip` can exercise it against a scratch home: the
/// data directory is a process-wide env var, and the lib's own test binary has
/// no lock to take.
pub async fn run_hook(command: &str, instance: &Instance) -> (bool, String) {
    let (shell, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
    let mut cmd = tokio::process::Command::new(shell);
    cmd.arg(flag)
        .arg(command)
        .current_dir(instance.game_dir())
        .env("INST_ID", &instance.id)
        .env("INST_NAME", &instance.name)
        .env("INST_DIR", instance.dir())
        .env("INST_MC_DIR", instance.game_dir())
        .env("INST_MC_VERSION", &instance.mc_version)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Dropping the child on a timeout has to kill it, or the hook outlives
        // the launch it was holding up.
        // ponytail: the shell is killed, not its own children -- a hook that
        // backgrounded something survives. Job objects and process groups if a
        // real hook ever leaks one.
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = async {
        match cmd.spawn() {
            Ok(child) => child.wait_with_output().await,
            Err(e) => Err(e),
        }
    };
    match tokio::time::timeout(HOOK_TIMEOUT, output).await {
        Err(_) => (
            false,
            format!("Killed after {} seconds.\n", HOOK_TIMEOUT.as_secs()),
        ),
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                text.push_str(&format!("Exited with {}.
", out.status));
            }
            (out.status.success(), text)
        }
        Ok(Err(e)) => (false, format!("Could not run {shell}: {e}
")),
    }
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
pub async fn launch(
    app: &AppHandle,
    mut instance: Instance,
    account: Account,
    quick_play: Option<QuickPlay>,
) -> Result<()> {
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
    let args = build_command(
        &version,
        &instance,
        &account,
        &cp.jars,
        &instance.natives_dir(),
        quick_play.as_ref(),
    )?;

    tokio::fs::create_dir_all(instance.game_dir()).await?;

    // Opened before the spawn, not after: a JVM that fails to start is exactly
    // the failure a user is asked to send this file for, and it used to reach
    // the UI only. Truncated per launch, like the game's own latest.log.
    let log_dir = instance.dir().join("logs");
    tokio::fs::create_dir_all(&log_dir).await?;
    let mut log = tokio::fs::File::create(log_dir.join("latest.log")).await?;
    let _ = log
        .write_all(format!("{java_bin}
{}

", args.join(" ")).as_bytes())
        .await;

    if !instance.pre_launch.is_empty() {
        let _ = log.write_all(b"Pre-launch command
").await;
        let (ok, output) = run_hook(&instance.pre_launch, &instance).await;
        let _ = log.write_all(output.as_bytes()).await;
        let _ = log.flush().await;
        if !ok {
            return Err(Error::msg(format!(
                "The pre-launch command failed, so the game was not started.
{}",
                output.trim()
            )));
        }
    }

    let mut command = tokio::process::Command::new(&java_bin);
    command
        .args(&args)
        .current_dir(instance.game_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let message = format!("Failed to start Java at {java_bin}: {e}");
            let _ = log.write_all(message.as_bytes()).await;
            let _ = log.flush().await;
            return Err(Error::msg(message));
        }
    };

    instance.last_played = instance::now_secs();
    instance.save().await?;
    let started = instance.last_played;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let id = instance.id.clone();

    let log_file = Arc::new(tokio::sync::Mutex::new(log));

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

        if !instance.post_exit.is_empty() {
            let (_, output) = run_hook(&instance.post_exit, &instance).await;
            let mut file = log_file.lock().await;
            let _ = file.write_all(b"Post-exit command
").await;
            let _ = file.write_all(output.as_bytes()).await;
            let _ = file.flush().await;
        }

        // Re-read rather than reusing the copy captured at launch: the settings
        // dialog may have written the file while the game was running.
        // Re-read also decides whether to count at all: the instance may have
        // opted out while the game was running.
        if let Ok(mut played) = instance::get(&id).await {
            if played.count_play_time {
                played.play_time += instance::now_secs().saturating_sub(started);
                let _ = played.save().await;
            }
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
    fn quick_play_is_offered_only_when_the_version_declares_it() {
        let mut version = VersionJson::default();
        assert!(!supports_quick_play(&version), "no arguments at all");

        version.arguments = Some(crate::mojang::Arguments {
            game: vec![Arg::Plain("--demo".into())],
            jvm: Vec::new(),
        });
        assert!(!supports_quick_play(&version), "pre-1.20 shape");

        let feature = |name: &str| crate::mojang::Rule {
            action: "allow".into(),
            os: None,
            features: HashMap::from([(name.to_string(), true)]),
        };
        version.arguments.as_mut().unwrap().game.push(Arg::Conditional {
            rules: vec![feature("is_demo_user")],
            value: crate::mojang::StringOrList::One("--demo".into()),
        });
        assert!(!supports_quick_play(&version), "some other feature rule");

        version.arguments.as_mut().unwrap().game.push(Arg::Conditional {
            rules: vec![feature("is_quick_play_multiplayer")],
            value: crate::mojang::StringOrList::One("--quickPlayMultiplayer".into()),
        });
        assert!(supports_quick_play(&version));
    }

    /// The window size is both a placeholder pair and a flag pair: modern
    /// metadata expands `${resolution_width}` behind a feature rule that never
    /// matches, so the flags have to be pushed here or the setting does
    /// nothing.
    #[test]
    fn a_window_size_reaches_the_command_line_only_when_it_is_set() {
        let mut version = VersionJson::default();
        version.main_class = "net.minecraft.client.main.Main".into();
        version.minecraft_arguments = Some("--width ${resolution_width}".into());
        let account = Account {
            id: "id".into(),
            name: "Notch".into(),
            kind: AccountKind::Offline,
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
            xuid: String::new(),
        };
        let mut instance = crate::instance::tests::sample();

        let args = build_command(&version, &instance, &account, &[], Path::new("n"), None).unwrap();
        assert!(!args.contains(&"--height".to_string()), "{args:?}");
        // Unset still expands the placeholder: Mojang's own default, not "0".
        assert!(args.contains(&"854".to_string()), "{args:?}");

        instance.window_width = 1920;
        instance.window_height = 1080;
        let args = build_command(&version, &instance, &account, &[], Path::new("n"), None).unwrap();
        let flag = args.iter().position(|a| a == "--height").expect("no --height");
        assert_eq!(args[flag + 1], "1080");
        assert!(args.contains(&"1920".to_string()), "{args:?}");
    }

    #[test]
    fn leaves_unknown_and_malformed_tokens_intact() {
        assert_eq!(substitute("${nope}", &vars()), "${nope}");
        assert_eq!(substitute("${unterminated", &vars()), "${unterminated");
        assert_eq!(substitute("a${nope}b${version_name}", &vars()), "a${nope}b1.21");
    }
}
