# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Minecraft launcher: Rust + Tauri 2 backend, Svelte 5 + Vite frontend. A
clean-slate rewrite of a Qt/C++ Prism Launcher fork; it does **not** read
Prism/MultiMC data. Scope is vanilla and Fabric launching, plus the mods folder
of a Fabric instance and installing mods from Modrinth — see README for what is
deliberately out of scope.

## Commands

```bash
pnpm install
pnpm tauri dev            # run with hot reload
pnpm tauri build          # release bundle (nsis/deb/appimage/dmg)

pnpm check                # svelte-check + tsc
pnpm test                 # frontend tests, node --test, no framework

cd src-tauri
cargo test                                        # offline tests only
cargo test maven_paths                            # one test by name
cargo test mojang::                               # one module
cargo test --test pack_roundtrip -- --test-threads=1
cargo test --test live_metadata -- --ignored --nocapture
cargo test --test live_jre -- --ignored
```

Network tests are `#[ignore]`d so `cargo test` stays offline and fast. Run
`live_metadata` after touching version resolution, the rule engine, or argument
building; it resolves real versions (modern, legacy 1.8.9, Fabric) and asserts
the built command line contains no unexpanded `${...}`.

Set `JUSTLAUNCHER_HOME` to a scratch directory to run the app or a test against
throwaway data instead of the user's real instances. Always do this when
manually exercising install or launch.

## Architecture

### The launch pipeline

`launch::launch` is the spine; everything else feeds it.

```
instance.json
  -> install::resolve      vanilla version JSON, then mojang::merge for Fabric
  -> install::classpath    Library::resolve per entry -> jars + natives + Jobs
  -> download::run         "Libraries", then "Assets"
  -> install::install      extract natives, materialise legacy asset trees
  -> java::find            a matching local JVM, else jre::ensure downloads one
  -> launch::build_command placeholder substitution
  -> spawn, pump stdout/stderr as `game-log` events and to logs/latest.log
```

`install::install` is idempotent and re-runs on every launch; it is the verify
pass, not a one-time step.

**One process per instance.** `RUNNING` in `launch.rs` maps instance id to the
`oneshot` that stops its process; `RunningGuard` claims the slot before the
install starts and releases it on `Drop`, so an error cannot wedge an instance.
Two games sharing one `.minecraft` corrupt saves, so a second launch is refused
rather than queued. `launch::stop` sends on that channel and the supervising
task kills the child. Anything that mutates an instance (Edit, Delete) is
disabled in the UI while a game runs.

**Refuse before spawning, not after.** A JVM mismatch or an impossible heap
surfaces as a Java crash the user cannot read, so `launch` checks first:
`java::is_compatible` (exact major, or newer only when the requirement is
already 17+), 32-bit JVMs against `-Xmx` over 2048 MB, and `memory_mb` against
`java::physical_memory_mb`. Unprobeable is not incompatible — only block on a
positive answer.

### Data layout

The filesystem is the database — no index, no cache. A user can copy, delete or
back up an instance with a file manager, and `instance::list` just reads
directories. See `paths.rs`.

```
<root>/instances/<id>/instance.json    id IS the folder name
                     /.minecraft       game working dir
                     /natives          rebuilt every install
                     /logs/latest.log  unredacted, truncated per launch
      /shared/{libraries,assets,versions,java}   deduplicated across instances
      /exports/<id>.zip
      /accounts.json
```

### Mojang metadata (`mojang.rs`)

The messiest domain, because Minecraft's format changed repeatedly:

- **Rules**: `rules_allow` evaluates OS/arch guards; last match wins, empty
  means allow. Used for both libraries and arguments. No optional features are
  declared, so feature-gated rules never match.
- **Natives**: legacy versions use a `natives` map into `downloads.classifiers`
  and must be extracted; modern ones name them `...:natives-windows` and put
  them on the classpath. `Library::resolve` distinguishes the two.
- **Arguments**: 1.13+ has structured `arguments`; older versions have a flat
  `minecraftArguments` string and expect the launcher to supply `-cp` and
  `-Djava.library.path` itself.
- **Assets**: three layouts. Modern reads the hash store directly; `virtual`
  (1.6–1.7.2) and `map_to_resources` (≤1.5.2) need a named-file tree built by
  `install::write_named_assets`.
- **Fabric** is a profile that inherits from a vanilla version. `mojang::merge`
  stacks it; child libraries go **first** so a patched copy shadows vanilla's.
  meta.fabricmc.net answers **400** for a Minecraft version Fabric never
  supported: that is an answer ("no loaders"), not a failure, and `fabric::loaders`
  translates it so the UI can say so before an instance is created. Its `stable`
  flag marks the **single** build Fabric currently recommends — every other
  loader is flagged false, so never present those as "unstable".

### Mods (`mods.rs`, `modrinth.rs`)

The mods folder is the database, like the instance list: `mods::list` reads
`.minecraft/mods` and nothing caches it. A mod is disabled by appending
`.disabled` to the file name — the convention every launcher shares, so the
user's other tools still understand the folder. Names come from the jar's
`fabric.mod.json`, falling back to the file name.

File names cross the IPC boundary and are joined onto a path, so `checked_name`
rejects separators, `..` and absolute paths before any of them touches the disk.

`modrinth.rs` is read-only, unauthenticated v2 API: search with a sort index and
category facet, project detail, version list, and the bulk update check. Every
download goes through the same `download::Job` as the rest of the launcher, so a
mod jar is SHA-1 verified like any library.

Two things worth knowing before touching it:

- **Identity is the hash, never the name.** Modrinth lists Jade as "Jade 🔍"
  while the jar inside calls itself "Jade", so any name comparison silently
  fails. `POST /v2/version_files` maps a jar's SHA-1 to the version and project
  it is, which is what "already installed" and "remove the old build" are both
  built on.
- **No local metadata index.** Prism writes a packwiz `.index` next to the jars
  to remember which project a file came from. We ask instead: `POST
  /v2/version_files/update` takes the SHA-1 of each installed jar and answers
  with the newest build. A jar Modrinth does not recognise is simply absent from
  the reply, which is exactly the right behaviour for a hand-built mod.
- **Required dependencies are installed with the mod.** `install_mod` walks
  `dependencies[]` breadth-first, filtered to `required`, with a `seen` set
  against diamonds. Missing Fabric API is the single most common reason a
  freshly installed mod crashes the game, so this is not optional polish.

Project descriptions are author-written markdown, so they are rendered through
`src/lib/markdown.ts` -- `marked` parses, DOMPurify decides what survives, and
neither is ever called without the other. It is the only place in the app that
uses `{@html}`.

CurseForge is not a second provider and cannot become one cheaply: its API needs
a per-launcher key that an open source build cannot ship, and some authors
forbid third-party downloads outright.

### Downloads (`download.rs`)

Everything fetched goes through `Job { url, path, sha1, size }`.

- Jobs are filtered by `is_valid` *before* downloading, so progress totals
  describe real work. A size match is accepted as proof without re-hashing —
  re-reading ~500 MB of assets per launch is not acceptable, and checksums are
  verified at download time.
- Temp files append `.part` rather than replacing the extension: a JRE manifest
  contains both `bin/java.exe` and `bin/java.dll`, which would otherwise
  collide.
- A ticker task emits `install-progress` every 200 ms with a smoothed rate;
  never emit per file.

### Java (`java.rs`, `jre.rs`)

`java::find` returns `Option` and never errors — when nothing matches,
`jre::ensure` downloads the runtime Mojang ships for that version. The version
JSON's `javaVersion.component` maps 1:1 onto Mojang's runtime manifest keys;
`component_for_major` only covers versions that name none. Downloaded runtimes
are discoverable by `java::discover`, so they appear in the settings picker.

### Auth (`auth.rs`)

OAuth device code flow, chosen so there is no embedded browser, no loopback
server and no client secret. Chain: Microsoft token → Xbox Live → XSTS →
Minecraft → profile. XSTS 401s carry an `XErr` code that must be translated —
those are the errors real users hit. There is no bundled Azure client ID
(`JUSTLAUNCHER_MSA_CLIENT_ID`), so offline accounts are the fallback path and
must keep working.

## Conventions

### The IPC contract is the wire format

Tauri converts **command arguments** from camelCase to snake_case
(`mcVersion` → `mc_version`), but **serialized struct fields** keep their serde
names. `#[serde(rename = "type")]` renames in *both* directions, so a field read
as `type` is also *written* as `type`. `invoke<T>()` is an unchecked assertion —
a mismatch is silently `undefined` at runtime. When changing a Rust type that
crosses the boundary, update `src/lib/api.ts` in the same edit.

### Scroll containers

Setting `overflow-y: auto` makes the browser compute `overflow-x` as `auto`
too, so a few pixels of overhang draw a scrollbar across a whole dialog. Every
scrolling container in the app pairs it with `overflow-x: hidden`; content that
genuinely cannot wrap (code blocks, tables) scrolls inside its own box, and a
table needs `display: block` before it will.

### Anything leaving the app gets redacted

Minecraft prints `(Session ID is token:...)` to stdout. `src/lib/redact.ts` is
the single chokepoint; route any new export, upload or clipboard path through
it. Redaction happens on the way **out**, so the live log view stays unmodified.
`logs/latest.log` is written unredacted because it never leaves the machine —
which is why `pack::is_skipped` excludes it from exported zips.

### Browsing is paged, filtered and racy

`Mods.svelte` pages Modrinth by **how many results it already holds**, never by
a page counter — appending then cannot drift out of step with the list. Each
search carries a run id so a slow earlier request cannot append to a newer
one's results, and "hide installed" tops the list up rather than leaving two
rows behind, because the installed set arrives after the first page and can
filter almost all of it away. Toggling the filter costs no request: the hits
are already held, only the derived view changes.

### One window for one long job

Downloads report through a single `install-progress` event, so the label has to
come from whoever started them: `task.begin`/`step`/`end` in
`src/lib/task.svelte.ts`, with `App.svelte` subscribing to the event once and
`TaskWindow.svelte` rendering whatever is running. The status strip describes
the selection, never the download.

### Instance actions live in one place

`src/lib/actions.ts` holds open-folder, export, stop and delete. The side panel,
the settings dialog and the right-click menu (`ContextMenu.svelte`) all call it;
adding an action there is what makes it appear everywhere it belongs.

### Dependencies

Deliberately few. The frontend has three: `marked` and
`dompurify` (paired, for Modrinth descriptions) and `jsdom` for their test.
`zip`, `reqwest`, `tokio`, `serde` are already present — reach
for those before adding anything. The frontend has no UI framework beyond
Svelte 5 runes and no SvelteKit (a desktop app needs no router); tests use
`node --test` rather than a test runner. The release profile is tuned for size
(`opt-level = "z"`, LTO, `panic = "abort"`, strip); keep the binary small.

### Untrusted input

Paths from downloaded manifests and zip archives are attacker-controlled.
Validate before joining: `enclosed_name()` for zip entries, explicit absolute
and `..` checks for manifest names. `instance::delete` removes a tree
recursively and guards its id accordingly.

### Verification

Non-trivial changes to the launch path are expected to be proven by actually
running the game, not only by tests — past work here caught real bugs
(placeholder expansion, temp-file collisions, wire-format mismatches) that unit
tests could not.
