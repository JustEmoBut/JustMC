//! Live check that a Mojang Java runtime can actually be resolved and fetched.
//!
//! Ignored by default: it downloads ~90 MB. Run with
//! `cargo test --test live_jre -- --ignored --nocapture`.

use justlauncher_lib::jre;

#[tokio::test]
#[ignore]
async fn every_component_minecraft_asks_for_is_published_for_this_platform() {
    // The components every supported Minecraft version names, plus the ones we
    // fall back to. If Mojang ever drops one for a platform, the launch path
    // silently degrades to "install a JDK yourself", so assert it up front.
    let wanted = [
        "jre-legacy",
        "java-runtime-alpha",
        "java-runtime-gamma",
        "java-runtime-delta",
        "java-runtime-epsilon",
    ];

    let available = jre::available_components().await.expect("fetch runtime index");
    println!("platform components: {available:?}");

    for component in wanted {
        assert!(
            available.contains(&component.to_string()),
            "Mojang no longer publishes {component} for this platform"
        );
    }
}

#[tokio::test]
#[ignore]
async fn plan_for_java_8_is_a_complete_runtime() {
    // 1.8.9 and friends: no `component` in the version JSON, so this exercises
    // the major-version fallback as well.
    let plan = jre::plan(None, 8).await.expect("plan jre-legacy");
    assert_eq!(plan.component, "jre-legacy");

    let bytes: u64 = plan.jobs.iter().filter_map(|j| j.size).sum();
    println!(
        "{}: {} files, {} MB -> {}",
        plan.component,
        plan.jobs.len(),
        bytes / 1024 / 1024,
        plan.binary().display()
    );

    assert!(plan.jobs.len() > 100, "too few files for a JRE");
    assert!(bytes > 20 * 1024 * 1024, "a JRE is bigger than this");
    assert!(
        plan.jobs.iter().all(|j| j.path.starts_with(&plan.dir)),
        "a manifest path escaped the install directory"
    );
    assert!(
        plan.jobs.iter().all(|j| j.sha1.is_some() && j.size.is_some()),
        "every runtime file should be checksummed and sized"
    );
    // The binary the launcher will invoke must be one of the files we fetch.
    assert!(
        plan.jobs.iter().any(|j| j.path == plan.binary()),
        "the manifest does not contain {}",
        plan.binary().display()
    );
}

#[tokio::test]
#[ignore]
async fn plan_honours_the_component_named_by_a_version_json() {
    let plan = jre::plan(Some("java-runtime-delta"), 21).await.unwrap();
    assert_eq!(plan.component, "java-runtime-delta");
}

#[tokio::test]
#[ignore]
async fn unknown_component_falls_back_to_the_major_version() {
    let plan = jre::plan(Some("java-runtime-nonexistent"), 17).await.unwrap();
    assert_eq!(plan.component, "java-runtime-gamma");
}
