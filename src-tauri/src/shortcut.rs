//! Desktop shortcuts that start the launcher with `--launch <id>`.
//!
//! The launcher owns no file format here: each platform's own kind of
//! shortcut is written to the desktop, pointing back at this executable.
//! `lib.rs` reads the argument at start-up and hands it to the frontend,
//! which launches the instance exactly as the Play button would.

use crate::error::{Error, Result};
use crate::instance::Instance;
use std::path::{Path, PathBuf};

/// The argument a shortcut passes, and the one `launch_arg` looks for.
pub const LAUNCH_FLAG: &str = "--launch";

/// The instance id a shortcut asked for, if the process was started by one.
pub fn launch_arg(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == LAUNCH_FLAG {
            return args.next().filter(|id| !id.is_empty());
        }
    }
    None
}

/// A file name for the shortcut: the instance's name with anything a
/// filesystem refuses replaced, so "Create: Above & Beyond" still works.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    let stem = stem.trim().trim_end_matches('.').to_string();
    if stem.is_empty() { "Minecraft".into() } else { stem }
}

/// Write a shortcut for `instance` on the desktop; returns its path.
pub async fn create(instance: &Instance) -> Result<PathBuf> {
    let desktop = dirs::desktop_dir().ok_or_else(|| Error::msg("This system has no desktop folder."))?;
    let exe = std::env::current_exe()?;
    let (stem, id) = (file_stem(&instance.name), instance.id.clone());
    tokio::task::spawn_blocking(move || write(&desktop, &stem, &exe, &id))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
}

#[cfg(windows)]
fn write(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    let path = desktop.join(format!("{stem}.lnk"));
    let args = format!("{LAUNCH_FLAG} {id}");
    lnk::save(&path, exe, &args).map_err(|hr| Error::msg(format!("Could not write the shortcut (HRESULT {hr:#010x}).")))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn write(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    // A `.command` is a shell script Finder runs on double click.
    let path = desktop.join(format!("{stem}.command"));
    std::fs::write(&path, format!("#!/bin/sh\nexec {} {LAUNCH_FLAG} {id}\n", sh_quote(exe)))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write(desktop: &Path, stem: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = desktop.join(format!("{stem}.desktop"));
    // The Exec key has its own quoting rules: a double-quoted argument with
    // `"`, `` ` ``, `$` and `\` escaped. The id is a slug and needs none.
    let exec: String = exe
        .to_string_lossy()
        .chars()
        .flat_map(|c| matches!(c, '"' | '`' | '$' | '\\').then_some('\\').into_iter().chain(Some(c)))
        .collect();
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName={stem}\nExec=\"{exec}\" {LAUNCH_FLAG} {id}\nTerminal=false\n"
        ),
    )?;
    // Most desktops only run an executable one ("trusted" launchers).
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// A `.lnk` through the shell's own `ShellLink` COM object, the only writer
/// Explorer is guaranteed to read back. `windows-sys` carries the functions
/// and the class id but no interface definitions, so the two vtables used are
/// declared here, in the order `ShObjIdl.h` and `ObjIdl.h` give them.
#[cfg(windows)]
mod lnk {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::core::{GUID, HRESULT};
    use windows_sys::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
    const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

    type Method = usize; // a slot this module never calls

    #[repr(C)]
    struct Unknown {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: Method,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    struct ShellLinkW {
        base: Unknown,
        get_path: Method,
        get_id_list: Method,
        set_id_list: Method,
        get_description: Method,
        set_description: Method,
        get_working_directory: Method,
        set_working_directory: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        get_arguments: Method,
        set_arguments: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        get_hotkey: Method,
        set_hotkey: Method,
        get_show_cmd: Method,
        set_show_cmd: Method,
        get_icon_location: Method,
        set_icon_location: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
        set_relative_path: Method,
        resolve: Method,
        set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    }

    #[repr(C)]
    struct PersistFile {
        base: Unknown,
        get_class_id: Method,
        is_dirty: Method,
        load: Method,
        save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
    }

    fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(Some(0)).collect()
    }

    fn check(hr: HRESULT) -> Result<(), HRESULT> {
        if hr < 0 { Err(hr) } else { Ok(()) }
    }

    /// Releases a COM pointer however `save` returns.
    struct Owned(*mut c_void);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a non-null pointer here came from CoCreateInstance or
                // QueryInterface and is released exactly once.
                unsafe { ((**(self.0 as *mut *const Unknown)).release)(self.0) };
            }
        }
    }

    /// Runs on a blocking-pool thread, which is initialised for COM here and
    /// uninitialised again before it goes back to the pool.
    pub fn save(path: &Path, target: &Path, args: &str) -> Result<(), HRESULT> {
        // SAFETY: plain COM calls with pointers into buffers that outlive them;
        // the vtables match the interfaces whose IIDs were asked for.
        unsafe {
            let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            check(init)?;
            let result = (|| {
                let mut link = Owned(std::ptr::null_mut());
                check(CoCreateInstance(&CLSID_SHELL_LINK, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ISHELL_LINK_W, &mut link.0))?;
                let vtable = &**(link.0 as *mut *const ShellLinkW);
                let exe = wide(target);
                check((vtable.set_path)(link.0, exe.as_ptr()))?;
                check((vtable.set_arguments)(link.0, wide(args).as_ptr()))?;
                check((vtable.set_icon_location)(link.0, exe.as_ptr(), 0))?;
                if let Some(dir) = target.parent() {
                    check((vtable.set_working_directory)(link.0, wide(dir).as_ptr()))?;
                }

                let mut file = Owned(std::ptr::null_mut());
                check((vtable.base.query_interface)(link.0, &IID_IPERSIST_FILE, &mut file.0))?;
                let persist = &**(file.0 as *mut *const PersistFile);
                check((persist.save)(file.0, wide(path).as_ptr(), 1))
            })();
            CoUninitialize();
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_argument_is_found_wherever_it_sits() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(launch_arg(args(&["app.exe", "--launch", "my-pack"])), Some("my-pack".into()));
        assert_eq!(launch_arg(args(&["app.exe", "--other", "--launch", "x"])), Some("x".into()));
        assert_eq!(launch_arg(args(&["app.exe", "--launch"])), None);
        assert_eq!(launch_arg(args(&["app.exe", "--launch", ""])), None);
        assert_eq!(launch_arg(args(&["app.exe"])), None);
    }

    /// The hand-declared vtables are the risk here: a slot out of order calls
    /// the wrong method. Explorer's own header is what a valid file starts with.
    #[cfg(windows)]
    #[test]
    fn a_real_lnk_is_written() {
        let dir = std::env::temp_dir().join("jl-test-shortcut");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = std::env::current_exe().unwrap();
        let path = write(&dir, "Test Pack", &exe, "test-pack").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // HeaderSize 0x4C, then the ShellLink CLSID.
        assert_eq!(&bytes[..4], &[0x4c, 0, 0, 0]);
        assert_eq!(&bytes[4..8], &[0x01, 0x14, 0x02, 0x00]);
        // The arguments are stored as UTF-16 text inside the file.
        let wide: Vec<u8> = "--launch test-pack".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(bytes.windows(wide.len()).any(|w| w == wide), "arguments missing");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_shortcut_name_survives_any_filesystem() {
        assert_eq!(file_stem("Create: Above & Beyond"), "Create_ Above & Beyond");
        assert_eq!(file_stem("a/b\\c?"), "a_b_c_");
        assert_eq!(file_stem("trailing."), "trailing");
        assert_eq!(file_stem("  "), "Minecraft");
    }
}
