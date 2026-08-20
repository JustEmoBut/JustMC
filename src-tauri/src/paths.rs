use std::path::PathBuf;

/// Everything the launcher owns lives under one root so a user can back it up
/// or move it with a single copy. Override with JUSTLAUNCHER_HOME.
pub fn root() -> PathBuf {
    if let Ok(p) = std::env::var("JUSTLAUNCHER_HOME") {
        return PathBuf::from(p);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("JustLauncher")
}

pub fn instances() -> PathBuf {
    root().join("instances")
}

/// Shared asset/library/version store — instances hard-reference it instead of
/// each keeping its own copy, which is where most of a launcher's disk use goes.
pub fn shared() -> PathBuf {
    root().join("shared")
}

pub fn libraries() -> PathBuf {
    shared().join("libraries")
}

pub fn assets() -> PathBuf {
    shared().join("assets")
}

pub fn versions() -> PathBuf {
    shared().join("versions")
}

/// Java runtimes downloaded from Mojang, shared across every instance.
pub fn java_runtimes() -> PathBuf {
    shared().join("java")
}

/// Where exported instance archives are written.
pub fn exports() -> PathBuf {
    root().join("exports")
}

pub fn accounts_file() -> PathBuf {
    root().join("accounts.json")
}

pub fn settings_file() -> PathBuf {
    root().join("settings.json")
}
