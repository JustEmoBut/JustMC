//! Export an instance, import it back, and check nothing was lost.
//!
//! Runs against a throwaway JUSTLAUNCHER_HOME so it never touches real data.

use justlauncher_lib::instance::{self, Instance, Loader};
use justlauncher_lib::pack;
use std::path::PathBuf;

/// Point the launcher at a private data directory for this test binary.
fn scratch_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jl-pack-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("JUSTLAUNCHER_HOME", &dir);
    dir
}

fn sample() -> Instance {
    Instance {
        id: "source".into(),
        name: "My Pack".into(),
        mc_version: "1.20.1".into(),
        loader: Loader::Fabric,
        loader_version: "0.16.0".into(),
        memory_mb: 6144,
        java_path: String::new(),
        jvm_args: "-XX:+UseG1GC".into(),
        last_played: 1_700_000_000,
        play_time: 7_200,
        installed: true,
    }
}

#[tokio::test]
async fn export_then_import_preserves_settings_and_world_data() {
    scratch_home();
    let original = sample();
    original.save().await.unwrap();

    // A save file and a config, i.e. the things a user would be upset to lose.
    let saves = original.game_dir().join("saves/world");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("level.dat"), b"world data").unwrap();
    std::fs::write(original.game_dir().join("options.txt"), b"fov:90").unwrap();

    // Natives are rebuilt on every launch and must not bloat the archive.
    std::fs::create_dir_all(original.natives_dir()).unwrap();
    std::fs::write(original.natives_dir().join("lwjgl.dll"), vec![0u8; 4096]).unwrap();

    let archive = pack::export(&original).unwrap();
    assert!(archive.exists(), "no archive written");

    let imported = pack::import(&archive).unwrap();

    // A fresh folder, so importing next to the original does not overwrite it.
    assert_ne!(imported.id, original.id);
    assert!(original.dir().exists(), "the source instance was disturbed");

    assert_eq!(imported.name, original.name);
    assert_eq!(imported.mc_version, original.mc_version);
    assert_eq!(imported.loader, original.loader);
    assert_eq!(imported.loader_version, original.loader_version);
    assert_eq!(imported.memory_mb, original.memory_mb);
    assert_eq!(imported.jvm_args, original.jvm_args);
    // Forced re-verify: this machine's shared store may lack the files.
    assert!(!imported.installed);

    assert_eq!(
        std::fs::read(imported.game_dir().join("saves/world/level.dat")).unwrap(),
        b"world data"
    );
    assert_eq!(
        std::fs::read(imported.game_dir().join("options.txt")).unwrap(),
        b"fov:90"
    );
    assert!(
        !imported.natives_dir().join("lwjgl.dll").exists(),
        "natives should not travel in the archive"
    );

    // The config on disk must carry the new id, or the folder and the config
    // disagree and every later lookup breaks.
    let on_disk = instance::get(&imported.id).await.unwrap();
    assert_eq!(on_disk.id, imported.id);

    let _ = std::fs::remove_dir_all(std::env::var("JUSTLAUNCHER_HOME").unwrap());
}

#[tokio::test]
async fn importing_something_that_is_not_an_instance_is_rejected() {
    let home = scratch_home();
    std::fs::create_dir_all(&home).unwrap();

    let bogus = home.join("not-an-instance.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&bogus).unwrap());
    zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();

    let err = pack::import(&bogus).unwrap_err().to_string();
    assert!(err.contains("instance.json"), "unhelpful error: {err}");
}

#[tokio::test]
async fn duplicate_copies_world_data_but_not_natives() {
    let dir = std::env::temp_dir().join(format!("jl-dup-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("JUSTLAUNCHER_HOME", &dir);

    let original = sample();
    original.save().await.unwrap();
    let saves = original.game_dir().join("saves/world");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("level.dat"), b"world data").unwrap();
    std::fs::create_dir_all(original.natives_dir()).unwrap();
    std::fs::write(original.natives_dir().join("lwjgl.dll"), b"native").unwrap();

    let copy = pack::duplicate(&original, "My Pack Copy").unwrap();

    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, "My Pack Copy");
    assert_eq!(copy.memory_mb, original.memory_mb);
    assert_eq!(copy.jvm_args, original.jvm_args);
    assert_eq!(copy.last_played, 0);
    // A copy has not been played; the source's hours are not the copy's.
    assert_eq!(copy.play_time, 0);
    assert!(!copy.installed, "the copy has no natives yet");
    assert_eq!(
        std::fs::read(copy.game_dir().join("saves/world/level.dat")).unwrap(),
        b"world data"
    );
    assert!(!copy.natives_dir().exists(), "natives were copied");
    assert!(original.dir().exists(), "the source instance was disturbed");
    // The config on disk carries the new id, not the source's.
    assert_eq!(instance::get(&copy.id).await.unwrap().id, copy.id);
}
