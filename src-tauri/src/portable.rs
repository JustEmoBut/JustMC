//! A launcher run as a bare exe rather than installed: it updates by
//! replacing its own file, not by running the NSIS installer, which would
//! install a second copy under Program Files and leave this one behind.

use crate::error::Result;
use std::path::{Path, PathBuf};

/// The `latest.json` platform key a portable build reads. The release
/// workflow adds this entry beside the installer's.
pub const UPDATER_TARGET: &str = "windows-x86_64-portable";

/// The running exe, when it is a portable copy. The NSIS installer always
/// writes `uninstall.exe` beside the exe it installs; a folder without one is
/// a copy someone put there.
pub fn exe() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    is_portable(exe.parent()?).then_some(exe)
}

fn is_portable(dir: &Path) -> bool {
    !dir.join("uninstall.exe").exists()
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Put `bytes` -- already verified against the release signature -- where
/// `exe` is. Windows refuses to overwrite a running exe but lets it be
/// renamed, so the running one steps aside to `.old` and the new one takes
/// its name; `clean_up` removes the old one on the next start. If the new
/// file cannot be put in place, the old one is moved back.
pub fn replace(exe: &Path, bytes: &[u8]) -> Result<()> {
    let new = sibling(exe, ".new");
    let old = sibling(exe, ".old");
    std::fs::write(&new, bytes)?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&new);
        return Err(e.into());
    }
    Ok(())
}

/// Remove what the last update left: the previous exe, no longer running.
pub fn clean_up() {
    if let Some(exe) = exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_copy_is_not_portable() {
        let dir = std::env::temp_dir().join("jl-test-portable");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("uninstall.exe"));
        assert!(is_portable(&dir));
        std::fs::write(dir.join("uninstall.exe"), b"").unwrap();
        assert!(!is_portable(&dir));
        let _ = std::fs::remove_file(dir.join("uninstall.exe"));
    }

    #[test]
    fn the_new_exe_takes_the_old_ones_place() {
        let dir = std::env::temp_dir().join("jl-test-replace");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("JustLauncher.exe");
        std::fs::write(&exe, b"old build").unwrap();
        replace(&exe, b"new build").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new build");
        assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old build");
        assert!(!sibling(&exe, ".new").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
