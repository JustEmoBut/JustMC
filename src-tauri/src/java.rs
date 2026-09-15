//! Finding a usable JVM. Minecraft pins a major version per release (8 for old
//! versions, 17 for 1.18+, 21 for 1.20.5+), so "a java on PATH" is not enough —
//! we probe candidates and match the required major.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct JavaInstall {
    pub path: String,
    pub version: String,
    pub major: u32,
    /// A 32-bit JVM cannot address more than ~2 GB of heap, whatever `-Xmx` says.
    pub bit64: bool,
}

fn exe_name() -> &'static str {
    // javaw has no console window on Windows, which is what we want for the
    // game process; the console output still reaches us through the pipes.
    if cfg!(windows) {
        "javaw.exe"
    } else {
        "java"
    }
}

/// Parse the major version out of `java -version` output, which looks like
/// `openjdk version "21.0.2" 2024-01-16` or, for Java 8, `"1.8.0_402"`.
fn parse_major(output: &str) -> Option<(String, u32)> {
    let quoted = output.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    let major = if first == 1 { parts.next()?.parse().ok()? } else { first };
    Some((quoted.to_string(), major))
}

fn probe(path: &Path) -> Option<JavaInstall> {
    // `java -version` writes to stderr, and javaw.exe on Windows writes nowhere
    // useful — probe with the console `java` binary next to it.
    let console = path.with_file_name(if cfg!(windows) { "java.exe" } else { "java" });
    let probe_bin = if console.exists() { console } else { path.to_path_buf() };

    let mut cmd = std::process::Command::new(&probe_bin);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let (version, major) = parse_major(&text)?;
    Some(JavaInstall {
        path: path.to_string_lossy().into_owned(),
        version,
        major,
        // Every 64-bit JVM says so in its banner ("64-Bit Server VM"); 32-bit
        // builds stay silent about it.
        bit64: text.contains("64-Bit"),
    })
}

/// Physical RAM in MB, or `None` when the platform will not say. Cached: it
/// never changes while we run, and `launch` asks on every start.
///
/// Windows answers through `GlobalMemoryStatusEx`; Linux and macOS have a file
/// and a sysctl. None of them is worth a system-facts crate for one number.
pub fn physical_memory_mb() -> Option<u32> {
    static CACHED: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(windows)]
        let bytes: u64 = {
            use windows_sys::Win32::System::SystemInformation::{
                GlobalMemoryStatusEx, MEMORYSTATUSEX,
            };
            let mut status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..unsafe { std::mem::zeroed() }
            };
            // SAFETY: the struct is zeroed and its dwLength set, which is the
            // whole contract; the call only writes into it.
            if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
                return None;
            }
            status.ullTotalPhys
        };

        #[cfg(not(windows))]
        let bytes: u64 = if cfg!(target_os = "linux") {
            let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
            let kb: u64 = meminfo
                .lines()
                .find_map(|l| l.strip_prefix("MemTotal:"))?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            kb * 1024
        } else if cfg!(target_os = "macos") {
            let out = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
            String::from_utf8_lossy(&out.stdout).trim().parse().ok()?
        } else {
            return None; // a platform none of this knows how to ask
        };
        u32::try_from(bytes / (1024 * 1024)).ok()
    })
}

/// Directories that commonly hold JDK installs, per platform.
fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("JAVA_HOME") {
        roots.push(PathBuf::from(home).join("bin"));
    }
    // Runtimes this launcher downloaded, so they show up in the settings picker
    // alongside system JDKs instead of being invisible.
    if let Ok(platforms) = std::fs::read_dir(crate::paths::java_runtimes()) {
        for platform in platforms.flatten() {
            if let Ok(components) = std::fs::read_dir(platform.path()) {
                for component in components.flatten() {
                    roots.push(component.path().join("bin"));
                    roots.push(component.path().join("jre.bundle/Contents/Home/bin"));
                }
            }
        }
    }
    if cfg!(windows) {
        for base in ["C:/Program Files/Java", "C:/Program Files/Eclipse Adoptium", "C:/Program Files/Microsoft/jdk", "C:/Program Files/Zulu"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                roots.extend(entries.flatten().map(|e| e.path().join("bin")));
            }
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(entries) = std::fs::read_dir("/Library/Java/JavaVirtualMachines") {
            roots.extend(entries.flatten().map(|e| e.path().join("Contents/Home/bin")));
        }
    } else {
        for base in ["/usr/lib/jvm", "/usr/lib64/jvm"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                roots.extend(entries.flatten().map(|e| e.path().join("bin")));
            }
        }
        roots.push(PathBuf::from("/usr/bin"));
    }
    roots
}

/// Every JVM we can find, deduplicated, newest major first.
pub fn discover() -> Vec<JavaInstall> {
    let mut found: Vec<JavaInstall> = search_roots()
        .into_iter()
        .map(|dir| dir.join(exe_name()))
        .filter(|p| p.exists())
        .filter_map(|p| probe(&p))
        .collect();

    // Whatever is on PATH, if we did not already reach it through a known root.
    if let Some(install) = probe(Path::new(exe_name())) {
        found.push(install);
    }

    found.sort_by(|a, b| b.major.cmp(&a.major).then(a.path.cmp(&b.path)));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

/// Pick a JVM for a version that requires `required` major, or `None` if this
/// machine has none — the caller then downloads one. An explicit user override
/// wins unconditionally; they may know something we do not.
pub fn find(override_path: Option<&str>, required: u32) -> Option<String> {
    if let Some(p) = override_path.filter(|p| !p.is_empty()) {
        return Some(p.to_string());
    }
    // The closest compatible major, not the newest one on the machine: a mod
    // loader is built against the Java the version asks for, and every release
    // past it is one more chance of a removed internal it reflects into. Forge
    // on a Java 26 JVM is the case that found this.
    discover()
        .iter()
        .filter(|j| is_compatible(j.major, required))
        .min_by_key(|j| j.major)
        .map(|j| j.path.clone())
}

/// Whether a JVM of major `have` can run a version that asks for `required`.
/// Newer JVMs usually work for older versions; Java 8 versions are the
/// exception, so only fall forward when the requirement is already modern.
pub fn is_compatible(have: u32, required: u32) -> bool {
    have == required || (required >= 17 && have > required)
}

/// The JVM at `path`, probed directly rather than through discovery.
pub fn inspect(path: &str) -> Option<JavaInstall> {
    probe(Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::{is_compatible, parse_major};

    #[test]
    fn only_modern_requirements_accept_a_newer_jvm() {
        assert!(is_compatible(8, 8));
        assert!(!is_compatible(21, 8)); // 1.8.9 breaks on modern JVMs
        assert!(!is_compatible(8, 17));
        assert!(is_compatible(21, 17));
        assert!(!is_compatible(17, 21));
    }

    #[test]
    fn the_closest_compatible_major_wins() {
        let mut majors: Vec<u32> = vec![26, 21, 17, 8]
            .into_iter()
            .filter(|m| is_compatible(*m, 21))
            .collect();
        majors.sort();
        assert_eq!(majors.first(), Some(&21));
    }

    #[test]
    fn parses_modern_and_legacy_output() {
        assert_eq!(
            parse_major("openjdk version \"21.0.2\" 2024-01-16").unwrap(),
            ("21.0.2".to_string(), 21)
        );
        assert_eq!(
            parse_major("java version \"1.8.0_402\"").unwrap(),
            ("1.8.0_402".to_string(), 8)
        );
        assert_eq!(parse_major("openjdk version \"17\"").unwrap().1, 17);
        assert!(parse_major("no version here").is_none());
    }
}

#[cfg(test)]
mod memory_tests {
    /// The one number the launcher refuses a launch over, so a platform that
    /// silently answers nothing would quietly disable that check.
    #[test]
    fn the_machine_reports_its_memory() {
        let mb = super::physical_memory_mb().expect("this platform reports RAM");
        assert!((512..=4 * 1024 * 1024).contains(&mb), "implausible: {mb} MB");
    }
}
