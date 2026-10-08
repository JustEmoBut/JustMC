# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Minecraft launcher: Rust + Tauri 2 backend, Svelte 5 + Vite frontend. A
clean-slate rewrite of a Qt/C++ Prism Launcher fork; it does **not** read
Prism/MultiMC data. Scope is vanilla, Fabric, Quilt, Forge and NeoForge
launching, plus the content folders of an instance, installing mods from
Modrinth or CurseForge and importing a modpack from either — see README for what is
deliberately out of scope.

## Commands

```bash
pnpm install
pnpm tauri dev            # run with hot reload
pnpm tauri build          # release bundle (nsis/deb/appimage/app/dmg)

pnpm check                # svelte-check + tsc
pnpm test                 # frontend tests, node --test, no framework

cd src-tauri
cargo test                                        # offline tests only
cargo test maven_paths                            # one test by name
cargo test mojang::                               # one module
cargo test --test pack_roundtrip                  # instance/pack tests
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

Per-module notes live in `src-tauri/CLAUDE.md`, frontend conventions in
`src/CLAUDE.md`; each loads when working under its directory.

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
  -> run_hook              the instance's pre-launch command, if it has one
  -> spawn, pump stdout/stderr as `game-log` events and to logs/latest.log
  -> run_hook              its post-exit command, after the process ends
```

`install::install` is idempotent and re-runs on every launch; it is the verify
pass, not a one-time step.

**A hook is a shell line, not an argument list.** `pre_launch` and `post_exit`
are handed to `cmd /C` or `sh -c` in the game directory, because a user writes
one the way they would type it in a terminal — quoting, `&&` and all — and
splitting the string here would only be a worse shell. The instance reaches it
through the environment (`INST_ID`, `INST_NAME`, `INST_DIR`, `INST_MC_DIR`,
`INST_MC_VERSION`), and both hooks' output lands in `logs/latest.log`. A
failed pre-launch command aborts the launch; a failed post-exit one is logged
and nothing more, since the session is already over.

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

**A version number is never a capability test.** Minecraft moved to a
year-based scheme in 2026, so an instance can be on `26.2`, and a snapshot
carries no minor version at all — "is the minor version at least 20" reads both
as older than 1.20. Ask the metadata what it declares instead:
`launch::supports_quick_play` looks for the `is_quick_play_*` feature rules the
version itself ships, and the `quick_play_supported` command exists so the
Servers and Worlds panels ask the same question the launch path does rather
than guessing in TypeScript. The frontend guessed once, and the Play button
silently never appeared.

`mojang::merge` extends the argument lists rather than replacing them, so a
loader profile that declares no Quick Play arguments still inherits vanilla's —
which is why the command resolves only `install::vanilla_version` and never
downloads a Forge installer to answer.

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
      /settings.json
```

### Settings (`settings.rs`)

`<root>/settings.json` holds the only preferences that belong to neither an
instance nor an account: what a **new** instance starts with (memory, Java, JVM
args), whether the window closes while a game runs, and the user's own
CurseForge API key (plain text; empty turns CurseForge off). Saving them never
touches an existing instance — that is what the instance window's Settings page is for.

Missing or unreadable falls back to defaults rather than erroring, and the
struct is `#[serde(default)]`, so a file from an older or newer build still
loads. The default heap is half the machine's RAM clamped to 2–8 GB, not a
constant: a flat 4 GB is refused outright by `launch` on an 8 GB machine and is
needlessly timid on a large one.

**The window closes for a game; it does not minimise.** The webview is ~95%
of the launcher's memory (measured: ~170 MB private across six WebView2
processes against 5 MB for the Rust side), and minimising frees none of it.
With `minimise_on_play` set (the name predates the change), `launch` destroys
the window right after the spawn, `WINDOW_CLOSED` makes the `ExitRequested`
handler in `lib.rs` keep the app alive, and the supervisor rebuilds the window
from `tauri.conf.json` once `RUNNING` is empty. The rebuilt UI starts fresh —
no live log, no exit toast; the run's output is in `logs/latest.log`. All of it
is backend-side because the frontend no longer exists when the game ends.

**Window calls are permission-gated.** `minimize`, `unminimize` and `setFocus`
are *not* in `core:window:default` — only the read-only `is-minimized` kind is.
A missing permission rejects the promise rather than erroring visibly, so any
new `getCurrentWindow()` call needs both an entry in
`capabilities/default.json` and a `.catch` that reports.

**Everything on the launch path is cached, so an installed instance launches
offline.** `version_json`, `asset_index` and Forge's profile were already
cache-first; `loader::profile` now is too, writing under the same version id
the instance launches. `mojang::manifest` is the exception that has to try the
network first — a new release has to appear in the picker — so it writes
`shared/versions/version_manifest.json` and falls back to it when the request
fails. That one request was the only thing standing between a fully installed
instance and playing with no connection.

**A window size is a placeholder pair *and* a flag pair.** `${resolution_width}`
expands from `window_width`/`window_height` (0 meaning "leave it to the game",
which is what options.txt already remembers), but the `--width`/`--height`
arguments that carry it sit behind a `has_custom_resolution` feature rule that
`rules_allow` never matches — so `build_command` pushes the flags itself, the
same arrangement Quick Play needs.

## Conventions

### The IPC contract is the wire format

Tauri converts **command arguments** from camelCase to snake_case
(`mcVersion` → `mc_version`), but **serialized struct fields** keep their serde
names. `#[serde(rename = "type")]` renames in *both* directions, so a field read
as `type` is also *written* as `type`. `invoke<T>()` is an unchecked assertion —
a mismatch is silently `undefined` at runtime. When changing a Rust type that
crosses the boundary, update `src/lib/api.ts` in the same edit.

### The CSP, and why `img-src` is wide

`default-src 'self'` governs everything the app loads; the directives that would
otherwise inherit it are pinned to `'none'` (objects, frames, workers, media,
`base-uri`, `form-action`) because the app uses none of them.

`img-src` is deliberately `https:` rather than an allowlist. The images the app
itself loads *are* a short list — `cdn.modrinth.com` for every project icon and
`textures.minecraft.net` for the skin an avatar is cut from (`Avatar.svelte`
draws it on a canvas; painting a cross-origin image needs no CORS, only
reading pixels back would) — but a Modrinth description is author-written
markdown and embeds images from wherever the author put them: a scan of 75
projects found 20+ hosts (imgur, raw.githubusercontent, jsdelivr, catbox,
personal domains). A fixed list cannot cover that, and CSP cannot tell a
description image from an app one. The accepted risk is a tracking pixel in a
description learning the reader's IP; there is no script or style consequence,
since those stay on `'self'`. Killing it means dropping `img` from
`markdown.ts`'s allowed tags, which costs the screenshots people choose a
shader by — a product decision, not a config tweak.

Do not add an explicit `connect-src`. Tauri's IPC rides a custom protocol whose
origin differs per platform, and the raw-bytes commands go the same way;
`default-src 'self'` already covers it.

### Anything leaving the app gets redacted

Minecraft prints `(Session ID is token:...)` to stdout. `src/lib/redact.ts` is
the single chokepoint; route any new export, upload or clipboard path through
it. Redaction happens on the way **out**, so the live log view stays unmodified.
`logs/latest.log` is written unredacted because it never leaves the machine —
which is why `pack::is_skipped` excludes it from exported zips.

### Dependencies

Deliberately few. The frontend has three: `marked` and
`dompurify` (paired, for Modrinth descriptions) and `jsdom` for their test.
`zip`, `reqwest`, `tokio`, `serde` are already present — reach
for those before adding anything. A few crates here carry real weight, each for one job that is
not a few lines: `hickory-resolver` (SRV lookups need DNS packets *and* the
system's resolver configuration on three platforms), `keyring` (three
different OS credential stores), `windows-sys` (already in the tree;
`GlobalMemoryStatusEx`, and COM's `CoCreateInstance` for the ShellLink object
`shortcut.rs` writes `.lnk` files with — the two interface vtables it needs are
declared there by hand, since `windows-sys` carries none),
`tauri-plugin-updater` (signed self-update; it brings a second `zip` major
and `tar`) and `tauri-plugin-single-instance` (registered first; a second
start hands its `--launch` over instead of becoming a second launcher that
could run an instance twice). Skin uploads build their multipart body by hand
rather than enabling reqwest's `multipart` feature. The frontend has no UI framework beyond
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
