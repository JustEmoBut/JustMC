//! Export an instance, import it back, and check nothing was lost.
//!
//! Runs against a throwaway JUSTLAUNCHER_HOME so it never touches real data.

use justlauncher_lib::instance::{self, Instance, Loader};
use justlauncher_lib::pack;
use std::io::Read;
use std::path::PathBuf;

/// The data directory is chosen by a process-wide env var, so only one test
/// can own it at a time. Holding this lock for the length of a test is what
/// lets plain `cargo test` run the binary's tests without them wiping each
/// other's home mid-run.
static HOME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Point the launcher at a private data directory, wiped first. Keep the
/// returned guard alive for the whole test: dropping it hands the directory
/// to the next one.
fn scratch_home() -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
    // A panicking test poisons the lock but leaves nothing to corrupt; the
    // next test wipes the directory anyway.
    let guard = HOME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("jl-pack-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("JUSTLAUNCHER_HOME", &dir);
    (dir, guard)
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
        count_play_time: true,
        installed: true,
    }
}

#[tokio::test]
async fn export_then_import_preserves_settings_and_world_data() {
    let _home = scratch_home();
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
    let (home, _guard) = scratch_home();
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
    let _home = scratch_home();

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

/// The New Instance dialog can pin a loader build; vanilla has none to pin.
#[tokio::test]
async fn a_pinned_loader_build_survives_creation_unless_there_is_no_loader() {
    let _home = scratch_home();

    let pinned = instance::create("Pinned", "1.21.1", Loader::Quilt, "0.24.0").await.unwrap();
    assert_eq!(pinned.loader_version, "0.24.0");
    // Read back, not just returned: the pin has to reach disk to survive.
    assert_eq!(instance::get(&pinned.id).await.unwrap().loader_version, "0.24.0");

    // Empty is the picker's default and means "resolve it at install time".
    let latest = instance::create("Latest", "1.21.1", Loader::Fabric, "").await.unwrap();
    assert!(latest.loader_version.is_empty());

    let vanilla = instance::create("Plain", "1.21.1", Loader::Vanilla, "0.24.0").await.unwrap();
    assert!(vanilla.loader_version.is_empty(), "vanilla has no loader to pin");
}

/// The world list, a backup and a delete, against a real instance on disk.
#[tokio::test]
async fn worlds_are_listed_backed_up_and_deleted() {
    let _home = scratch_home();
    let instance = sample();
    instance.save().await.unwrap();

    // level.dat is gzipped NBT; the name and the last-played time come out of
    // it rather than off the file.
    use justlauncher_lib::nbt::Tag;
    let saves = instance.game_dir().join("saves/New World");
    std::fs::create_dir_all(saves.join("region")).unwrap();
    let level = Tag::Compound(vec![(
        "Data".to_string(),
        Tag::Compound(vec![
            ("LevelName".to_string(), Tag::String("Ev Dunyam".into())),
            ("LastPlayed".to_string(), Tag::Long(1_700_000_000_000)),
        ]),
    )]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &justlauncher_lib::nbt::write("", &level)).unwrap();
    std::fs::write(saves.join("level.dat"), gz.finish().unwrap()).unwrap();
    std::fs::write(saves.join("region/r.0.0.mca"), vec![7u8; 2048]).unwrap();
    // Not a world: no level.dat, so it must not be listed or deletable.
    std::fs::create_dir_all(instance.game_dir().join("saves/notaworld")).unwrap();

    let worlds = justlauncher_lib::worlds::list(&instance.id).await.unwrap();
    assert_eq!(worlds.len(), 1, "only folders holding a level.dat are worlds");
    assert_eq!(worlds[0].folder, "New World");
    assert_eq!(worlds[0].name, "Ev Dunyam");
    // The file's own LastPlayed, not the mtime of the file just written.
    assert_eq!(worlds[0].last_played, 1_700_000_000);
    assert!(worlds[0].size > 2048, "size covers the whole tree");

    let archive = justlauncher_lib::worlds::backup(&instance.id, "New World").await.unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_owned()).collect();
    assert!(names.contains(&"New World/level.dat".to_string()), "{names:?}");
    assert!(names.contains(&"New World/region/r.0.0.mca".to_string()), "{names:?}");

    assert!(justlauncher_lib::worlds::delete(&instance.id, "notaworld").await.is_err());
    assert!(justlauncher_lib::worlds::delete(&instance.id, "../../accounts.json").await.is_err());
    justlauncher_lib::worlds::delete(&instance.id, "New World").await.unwrap();
    assert!(!saves.exists());
    assert!(justlauncher_lib::worlds::list(&instance.id).await.unwrap().is_empty());
}

/// Unpacked pack folders: only the ones the game itself would read are listed,
/// and deleting one takes the whole tree with it.
#[tokio::test]
async fn unpacked_pack_folders_are_listed_and_deleted() {
    use justlauncher_lib::mods::{self, Kind};
    let _home = scratch_home();
    let instance = sample();
    instance.save().await.unwrap();

    let packs = instance.game_dir().join("resourcepacks");
    std::fs::create_dir_all(packs.join("Faithful/assets")).unwrap();
    std::fs::write(
        packs.join("Faithful/pack.mcmeta"),
        br#"{"pack":{"pack_format":15,"description":"32x textures"}}"#,
    )
    .unwrap();
    std::fs::write(packs.join("Faithful/assets/a.png"), vec![1u8; 16]).unwrap();
    // No pack.mcmeta, so not a pack: never listed, never deletable from here.
    std::fs::create_dir_all(packs.join("old backup")).unwrap();

    let listed = mods::list(&instance.id, Kind::Resourcepacks).await.unwrap();
    assert_eq!(listed.len(), 1, "only folders the game reads are packs");
    assert_eq!(listed[0].file, "Faithful");
    assert_eq!(listed[0].version, "32x textures");
    assert!(listed[0].dir && listed[0].enabled);
    assert!(listed[0].sha1.is_empty(), "a folder has no jar to match on Modrinth");

    // Shaders are recognised by a shaders/ directory instead.
    let shaders = instance.game_dir().join("shaderpacks");
    std::fs::create_dir_all(shaders.join("BSL/shaders")).unwrap();
    let listed = mods::list(&instance.id, Kind::Shaderpacks).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "BSL");

    let disabled = mods::set_enabled(&instance.id, Kind::Resourcepacks, "Faithful", false)
        .await
        .unwrap();
    assert_eq!(disabled, "Faithful.disabled");
    let listed = mods::list(&instance.id, Kind::Resourcepacks).await.unwrap();
    assert_eq!(listed[0].name, "Faithful", "the suffix is not part of the name");
    assert!(!listed[0].enabled);

    mods::delete(&instance.id, Kind::Resourcepacks, &disabled).await.unwrap();
    assert!(!packs.join("Faithful.disabled").exists());
    assert!(packs.join("old backup").exists(), "an unlisted folder is untouched");
}

/// A `.mrpack` export: the index describes the pack, everything else rides in
/// `overrides/`, and the player's own files stay home.
///
/// The instance deliberately holds no top-level content files, so the Modrinth
/// lookup is empty and the test stays offline.
#[tokio::test]
async fn an_mrpack_export_separates_the_pack_from_the_player() {
    use justlauncher_lib::mrpack;

    let _home = scratch_home();
    let original = sample();
    original.save().await.unwrap();

    let game = original.game_dir();
    std::fs::create_dir_all(game.join("config")).unwrap();
    std::fs::write(game.join("config/sodium.toml"), b"enabled=true").unwrap();
    // An unpacked shader folder is pack content, but not index material.
    std::fs::create_dir_all(game.join("shaderpacks/BSL/shaders")).unwrap();
    std::fs::write(game.join("shaderpacks/BSL/shaders/final.fsh"), b"shader").unwrap();
    // The player's own: a world, their keybinds, their server list.
    std::fs::create_dir_all(game.join("saves/New World")).unwrap();
    std::fs::write(game.join("saves/New World/level.dat"), b"world data").unwrap();
    std::fs::write(game.join("options.txt"), b"fov:90").unwrap();
    std::fs::write(game.join("servers.dat"), b"servers").unwrap();

    let archive = mrpack::export(&original).await.unwrap();
    assert!(archive.to_string_lossy().ends_with("source.mrpack"), "{}", archive.display());

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_owned()).collect();

    assert!(names.contains(&"modrinth.index.json".to_string()), "{names:?}");
    assert!(names.contains(&"overrides/config/sodium.toml".to_string()), "{names:?}");
    assert!(names.contains(&"overrides/shaderpacks/BSL/shaders/final.fsh".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("saves") || n.contains("options.txt") || n.contains("servers.dat")), "{names:?}");

    // The index is what every other launcher reads first; its shape is the
    // contract, so assert it rather than trust the writer's types.
    let mut text = String::new();
    zip.by_name("modrinth.index.json").unwrap().read_to_string(&mut text).unwrap();
    let index: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(index["formatVersion"], 1);
    assert_eq!(index["game"], "minecraft");
    assert_eq!(index["name"], "My Pack");
    assert_eq!(index["dependencies"]["minecraft"], "1.20.1");
    assert_eq!(index["dependencies"]["fabric-loader"], "0.16.0");
    assert_eq!(index["files"].as_array().unwrap().len(), 0, "nothing to resolve offline");

    let _ = std::fs::remove_dir_all(std::env::var("JUSTLAUNCHER_HOME").unwrap());
}

/// The server list: read what the game wrote, add one, remove one, and leave
/// the keys this launcher does not understand alone.
#[tokio::test]
async fn servers_are_listed_added_and_removed() {
    use justlauncher_lib::nbt::{self, Tag};
    use justlauncher_lib::servers;

    let _home = scratch_home();
    let instance = sample();
    instance.save().await.unwrap();

    // What the game itself leaves behind, including a key we never write.
    let existing = Tag::Compound(vec![(
        "servers".to_string(),
        Tag::List(
            10,
            vec![Tag::Compound(vec![
                ("name".to_string(), Tag::String("Hypixel".into())),
                ("ip".to_string(), Tag::String("mc.hypixel.net".into())),
                ("acceptTextures".to_string(), Tag::Byte(1)),
            ])],
        ),
    )]);
    let file = instance.game_dir().join("servers.dat");
    std::fs::create_dir_all(instance.game_dir()).unwrap();
    std::fs::write(&file, nbt::write("", &existing)).unwrap();

    let list = servers::list(&instance.id).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Hypixel");
    assert_eq!(list[0].ip, "mc.hypixel.net");

    servers::add(&instance.id, "Home", "192.168.1.10:25566").await.unwrap();
    let list = servers::list(&instance.id).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!((list[1].index, list[1].ip.as_str()), (1, "192.168.1.10:25566"));

    // The game's own key survived the rewrite.
    let (_, root) = nbt::read(&std::fs::read(&file).unwrap()).unwrap();
    let first = &root.get("servers").unwrap();
    let Tag::List(_, items) = first else { panic!("servers is not a list") };
    assert_eq!(items[0].get("acceptTextures"), Some(&Tag::Byte(1)));

    // A position whose address no longer matches deletes nothing.
    assert!(servers::remove(&instance.id, 1, "mc.hypixel.net").await.is_err());
    assert!(servers::remove(&instance.id, 9, "whatever").await.is_err());
    assert_eq!(servers::list(&instance.id).await.unwrap().len(), 2);

    servers::remove(&instance.id, 0, "mc.hypixel.net").await.unwrap();
    let list = servers::list(&instance.id).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].ip, "192.168.1.10:25566");

    // An address is the one thing an entry cannot do without.
    assert!(servers::add(&instance.id, "Nowhere", "  ").await.is_err());
}

/// The account store, against the machine's real keychain. Ignored by default
/// because it writes to the OS credential store; the point of the feature is
/// that the tokens are not in the file, and only a real store proves it.
#[tokio::test]
#[ignore = "touches the OS keychain"]
async fn tokens_live_in_the_keychain_not_the_file() {
    use justlauncher_lib::auth::{self, Account, AccountKind};

    let (home, _guard) = scratch_home();
    let id = "jl-test-account-please-delete";
    auth::forget(id).await;

    let account = Account {
        id: id.to_string(),
        name: "Tester".into(),
        kind: AccountKind::Microsoft,
        access_token: "access-secret".into(),
        refresh_token: "refresh-secret".into(),
        expires_at: 1_700_000_000,
        xuid: "1234".into(),
    };
    auth::save_all(std::slice::from_ref(&account)).await.unwrap();

    let file = std::fs::read_to_string(home.join("accounts.json")).unwrap();
    assert!(!file.contains("access-secret"), "the file still holds a token: {file}");
    assert!(!file.contains("refresh-secret"), "the file still holds a token: {file}");
    assert!(file.contains("Tester"), "the file lost the account itself");

    let back = auth::load_all().await;
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].access_token, "access-secret");
    assert_eq!(back[0].refresh_token, "refresh-secret");
    assert_eq!(back[0].expires_at, 1_700_000_000);

    // Forgetting one leaves the account readable but its secrets gone.
    auth::forget(id).await;
    let back = auth::load_all().await;
    assert_eq!(back.len(), 1);
    assert!(back[0].refresh_token.is_empty());
}

/// An `accounts.json` from before the keychain: the tokens are read, then
/// moved out of the file the first time anything looks at it.
#[tokio::test]
#[ignore = "touches the OS keychain"]
async fn a_plaintext_file_is_migrated_on_first_read() {
    use justlauncher_lib::auth;

    let (home, _guard) = scratch_home();
    let id = "jl-test-legacy-account-please-delete";
    auth::forget(id).await;

    let path = home.join("accounts.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            r#"[{{"id":"{id}","name":"Old","kind":"microsoft","access_token":"old-access","refresh_token":"old-refresh","expires_at":5,"xuid":"9"}}]"#
        ),
    )
    .unwrap();

    let loaded = auth::load_all().await;
    assert_eq!(loaded[0].refresh_token, "old-refresh", "the login must survive the move");

    let file = std::fs::read_to_string(&path).unwrap();
    assert!(!file.contains("old-refresh"), "the file was not migrated: {file}");
    assert_eq!(auth::load_all().await[0].refresh_token, "old-refresh");

    auth::forget(id).await;
}

