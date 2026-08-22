//! End-to-end checks against the real Mojang and Fabric metadata services.
//!
//! Ignored by default so the normal `cargo test` run stays offline and fast.
//! Run with `cargo test --test live_metadata -- --ignored --nocapture` when
//! touching version resolution, the rule engine or argument building — those
//! are the parts that break silently against real-world metadata.

use justlauncher_lib::auth::offline_account;
use justlauncher_lib::loader;
use justlauncher_lib::install::{classpath, resolve};
use justlauncher_lib::instance::{Instance, Loader};
use justlauncher_lib::launch::build_command;

fn instance(mc_version: &str, loader: Loader, loader_version: &str) -> Instance {
    Instance {
        id: "live-test".into(),
        name: "Live Test".into(),
        mc_version: mc_version.into(),
        loader,
        loader_version: loader_version.into(),
        memory_mb: 4096,
        java_path: String::new(),
        jvm_args: "-XX:+UseG1GC".into(),
        last_played: 0,
        play_time: 0,
        installed: false,
    }
}

async fn check(inst: Instance) -> Vec<String> {
    let version = resolve(&inst).await.expect("resolve version");
    let cp = classpath(&version, &inst.mc_version).expect("build classpath");

    assert!(!version.main_class.is_empty(), "no main class");
    assert!(cp.jars.len() > 5, "suspiciously few classpath entries");
    assert!(!cp.jobs.is_empty(), "nothing to download");
    assert!(version.asset_index.is_some(), "no asset index");

    let account = offline_account("TestPlayer").unwrap();
    let args = build_command(
        &version,
        &inst,
        &account,
        &cp.jars,
        std::path::Path::new("/tmp/natives"),
    )
    .expect("build command");

    // Any leftover placeholder means a token the launcher failed to supply,
    // which shows up in-game as a broken session rather than a crash.
    let leftover: Vec<_> = args.iter().filter(|a| a.contains("${")).collect();
    assert!(leftover.is_empty(), "unexpanded placeholders: {leftover:?}");

    assert!(args.contains(&"TestPlayer".to_string()), "player name missing");
    assert!(args.iter().any(|a| a == "-Xmx4096M"), "heap size missing");
    assert!(args.contains(&version.main_class), "main class missing");
    args
}

#[tokio::test]
#[ignore]
async fn modern_vanilla_resolves_and_builds_args() {
    let args = check(instance("1.21.4", Loader::Vanilla, "")).await;
    println!("vanilla args: {}", args.join(" "));
}

#[tokio::test]
#[ignore]
async fn legacy_vanilla_uses_flat_argument_string() {
    // 1.8.9 has `minecraftArguments` instead of `arguments`, and legacy natives
    // jars with OS classifiers — a completely different code path.
    let args = check(instance("1.8.9", Loader::Vanilla, "")).await;
    assert!(
        args.iter().any(|a| a.starts_with("-Djava.library.path=")),
        "legacy versions need an explicit natives path"
    );
    println!("legacy args: {}", args.join(" "));
}

#[tokio::test]
#[ignore]
async fn loader_profiles_merge_onto_vanilla() {
    // Both loaders merge the same way and both launch through Knot, so the one
    // check covers the pair; a Quilt-only regression would show up here.
    for kind in [Loader::Fabric, Loader::Quilt] {
        let version = loader::latest_loader(kind, "1.21.4").await.expect("a loader build");
        let args = check(instance("1.21.4", kind, &version)).await;
        assert!(
            args.iter().any(|a| a.contains("knot")),
            "{kind:?} should replace the main class with its Knot launcher"
        );
        println!("{kind:?} loader {version}: {}", args.join(" "));
    }
}

/// The three asset layouts Minecraft has used. Getting these wrong is silent:
/// the game starts and simply has no sounds or language files.
#[tokio::test]
#[ignore]
async fn legacy_asset_layouts_are_detected() {
    use justlauncher_lib::mojang;

    for (version, expect_virtual, expect_resources) in [
        ("1.5.2", false, true),  // pre-1.6: copied into .minecraft/resources
        ("1.7.2", true, false),  // legacy: copied into assets/virtual/legacy
        ("1.8.9", false, false), // modern: read straight from the hashed store
    ] {
        let resolved = resolve(&instance(version, Loader::Vanilla, "")).await.unwrap();
        let index_ref = resolved.asset_index.as_ref().expect("no asset index");
        let index = mojang::asset_index(index_ref).await.unwrap();

        println!(
            "{version}: index={} virtual={} map_to_resources={} objects={}",
            index_ref.id,
            index.is_virtual,
            index.map_to_resources,
            index.objects.len()
        );

        assert_eq!(index.is_virtual, expect_virtual, "{version} virtual flag");
        assert_eq!(
            index.map_to_resources, expect_resources,
            "{version} map_to_resources flag"
        );
        assert_eq!(
            index.needs_named_copy(),
            expect_virtual || expect_resources,
            "{version} reconstruction decision"
        );
        assert!(!index.objects.is_empty());
        assert_eq!(index.jobs().len(), index.objects.len());
    }
}

/// A version Fabric never supported answers 400, which must read as "no
/// loaders" rather than a network failure.
#[tokio::test]
#[ignore = "network"]
async fn fabric_reports_no_loaders_for_an_unsupported_version() {
    assert!(loader::loaders(Loader::Fabric, "1.12.2").await.unwrap().is_empty());
    assert!(!loader::loaders(Loader::Fabric, "1.21.1").await.unwrap().is_empty());
}

/// Modrinth search and version resolution, against the real API.
#[tokio::test]
#[ignore = "network"]
async fn modrinth_finds_a_fabric_mod_and_its_jar() {
    use justlauncher_lib::modrinth;

    let page = modrinth::search("sodium", Some("1.21.1"), "relevance", None, 0, 5, "mod", &["fabric"])
        .await
        .unwrap();
    assert!(!page.hits.is_empty(), "sodium should be findable for 1.21.1");

    let version = modrinth::latest_version(&page.hits[0].project_id, "1.21.1", &["fabric"])
        .await
        .unwrap();
    let jar = version.jar().expect("a version has a jar");
    assert!(jar.filename.ends_with(".jar"), "got {}", jar.filename);
    assert!(jar.size > 0);
    assert!(jar.hashes.sha1.is_some());

    // A version the project has no Fabric build for must say so, not panic.
    assert!(
        modrinth::latest_version(&page.hits[0].project_id, "1.2.5", &["fabric"])
            .await
            .is_err()
    );
}

/// Search sorting, project detail, version listing and the hash-based update
/// check, against the real Modrinth API.
#[tokio::test]
#[ignore = "network"]
async fn modrinth_search_sort_and_update_check() {
    use justlauncher_lib::modrinth;

    let page = modrinth::search(
        "",
        Some("1.21.1"),
        "downloads",
        Some("optimization"),
        0,
        5,
        "mod",
        &["fabric"],
    )
    .await
    .unwrap();
    assert_eq!(page.hits.len(), 5);
    assert!(page.total_hits > 5);
    // Sorted by downloads, so each hit is at most as popular as the one before.
    for pair in page.hits.windows(2) {
        assert!(pair[0].downloads >= pair[1].downloads);
    }

    let project = modrinth::project(&page.hits[0].project_id).await.unwrap();
    assert!(!project.title.is_empty());

    let versions = modrinth::versions(&project.id, "1.21.1", &["fabric"]).await.unwrap();
    let jar = versions[0].jar().expect("a version has a jar");
    let sha1 = jar.hashes.sha1.clone().expect("modrinth publishes sha1");

    // The file we sent is known, so it comes back; a made-up hash does not.
    let map = modrinth::updates(&[sha1.clone(), "0".repeat(40)], "1.21.1", &["fabric"])
        .await
        .unwrap();
    assert!(map.contains_key(&sha1));
    assert_eq!(map.len(), 1);
}

/// A mod with required dependencies resolves them; Fabric API is the usual one.
#[tokio::test]
#[ignore = "network"]
async fn modrinth_reports_required_dependencies() {
    use justlauncher_lib::modrinth;

    let version = modrinth::latest_version("rei", "1.21.1", &["fabric"]).await.unwrap();
    let required: Vec<_> = version
        .dependencies
        .iter()
        .filter(|d| d.dependency_type == "required")
        .collect();
    assert!(!required.is_empty(), "REI requires Fabric API and more");
    assert!(required.iter().all(|d| d.project_id.is_some() || d.version_id.is_some()));
}

/// Installed jars are identified by hash, not by name: Modrinth lists Jade as
/// "Jade 🔍" while the jar inside calls itself "Jade".
#[tokio::test]
#[ignore = "network"]
async fn modrinth_identifies_a_jar_by_its_hash() {
    use justlauncher_lib::modrinth;

    let version = modrinth::latest_version("jade", "1.21.1", &["fabric"]).await.unwrap();
    let sha1 = version.jar().unwrap().hashes.sha1.clone().unwrap();

    let found = modrinth::version_files(&[sha1.clone(), "0".repeat(40)]).await.unwrap();
    assert_eq!(found.len(), 1, "an unknown hash must not come back");
    assert_eq!(found[&sha1].project_id, version.project_id);

    let project = modrinth::project(&version.project_id).await.unwrap();
    assert_ne!(project.title, "Jade", "title differs from the jar's own name");
}

/// Resource packs and shaders come from the same API as mods but are tagged
/// differently: a different project type, and loaders that are not "fabric".
/// Getting either wrong returns an empty catalogue rather than an error, so
/// this asserts real results come back.
#[tokio::test]
#[ignore = "network"]
async fn modrinth_serves_resource_packs_and_shaders() {
    use justlauncher_lib::mods::Kind;
    use justlauncher_lib::modrinth;

    for kind in [Kind::Resourcepacks, Kind::Shaderpacks] {
        let page = modrinth::search("", Some("1.21.1"), "downloads", None, 0, 5, kind.project_type(), &[])
            .await
            .unwrap();
        assert!(!page.hits.is_empty(), "no {} for 1.21.1", kind.project_type());

        let versions = modrinth::versions(&page.hits[0].project_id, "1.21.1", kind.loaders(Loader::Vanilla))
            .await
            .unwrap();
        assert!(!versions.is_empty(), "{} has no build for 1.21.1", page.hits[0].slug);

        // Both kinds ship a zip, which is what `Kind::extension` expects on disk.
        let file = versions[0].jar().expect("a version has a file");
        assert!(file.filename.to_lowercase().ends_with(".zip"), "got {}", file.filename);
        assert!(file.hashes.sha1.is_some());
    }
}

/// Quilt, whose metadata is the same shape as Fabric's but differs in the
/// three details `loader.rs` documents: 404 rather than 400 for a version it
/// never supported, no `stable` flag, and an unordered list.
#[tokio::test]
#[ignore = "network"]
async fn quilt_lists_loaders_and_publishes_a_profile() {
    // Never supported: an answer, not a failure.
    assert!(loader::loaders(Loader::Quilt, "1.12.2").await.unwrap().is_empty());

    let list = loader::loaders(Loader::Quilt, "1.21.1").await.unwrap();
    assert!(!list.is_empty(), "quilt supports 1.21.1");
    // Quilt does not sort its own list, so this asserts ours is sorted: taking
    // the server's first entry pinned an old beta.
    let newest = &list[0].version;
    assert!(
        list.iter().all(|l| l.version.as_str() <= newest.as_str() || l.version.contains('-')),
        "list is not newest-first: {newest} then {}",
        list[1].version
    );
    // Quilt flags nothing as recommended, and none is invented for it.
    assert!(list.iter().all(|l| !l.stable));

    let version = loader::latest_loader(Loader::Quilt, "1.21.1").await.unwrap();
    let profile = loader::profile(Loader::Quilt, "1.21.1", &version).await.unwrap();
    assert_eq!(profile.id, format!("quilt-loader-{version}-1.21.1"));
    assert_eq!(profile.inherits_from.as_deref(), Some("1.21.1"));
    assert!(profile.main_class.contains("quilt"), "got {}", profile.main_class);
    assert!(!profile.libraries.is_empty());
}
