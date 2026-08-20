# JustLauncher

A small, fast Minecraft launcher. Rust + Tauri 2 + Svelte 5.

Rewrite of the Qt/C++ [JustLauncher](https://github.com/JustEmoBut/JustLauncher)
(a Prism Launcher fork). This version is a clean-slate implementation — it does
not read Prism/MultiMC instance data.

| | Qt version | This version |
|---|---|---|
| Executable | ~15 MB + Qt runtime | **4.3 MB**, no runtime to ship |
| Installer | ~40 MB | **1.7 MB** |
| Idle memory | ~120 MB | **~28 MB** |

The webview is the OS's own (WebView2 / WebKitGTK), so nothing like Electron's
Chromium is bundled.

## What it does

- Microsoft account sign-in (OAuth device code flow) and offline accounts
- Full vanilla version list, releases and snapshots, back to the oldest versions
- Fabric and Quilt loaders, with the loader build pinnable when an instance is
  created and afterwards
- Create, configure, delete instances; per-instance memory, Java and JVM args
- Parallel downloading with SHA-1 verification, resumable across runs, live
  transfer rate and ETA
- Shared library/asset store, so instances on the same version cost no extra disk
- Every asset layout Minecraft has used, including the named-file trees that
  1.7.2 and older need
- Mod management for Fabric instances, and resource packs and shaders for any
  instance: list, enable, disable, delete, drop a file on the window, and check
  what is installed for newer builds
- Browse Modrinth from inside the launcher — mods, resource packs and shaders:
  sort by relevance, downloads, followers or date, filter by category, hide what
  is already installed, read a project and install any version of it, with its
  required dependencies
- Export an instance to a zip; import one, or a Modrinth `.mrpack` modpack,
  from Add Instance or by dropping it on the window
- Duplicate an instance with its worlds and configs; change an existing
  instance's Minecraft version
- Java detection matched to each version's required major, and automatic
  download of Mojang's own runtime when the machine has none
- Total play time per instance, and launcher settings: the memory, Java and JVM
  arguments a new instance starts with, and whether the window minimises while
  the game runs
- Live game log with error highlighting; copying it redacts session tokens and
  usernames

## Not included

CurseForge, world manager, custom themes, Forge/NeoForge, skin management.

CurseForge is absent for a reason rather than an oversight: its API needs a
per-launcher key that cannot be shipped in an open source build, and it lets
authors forbid third-party downloads, which turns "install" into "open a browser
and do it yourself" for a slice of the catalogue. Modrinth needs no key and
allows all of it.

## Building

Needs [Rust](https://rustup.rs), Node 20+, pnpm, and the platform's
[Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
pnpm install
pnpm tauri dev      # development
pnpm tauri build    # release bundle
```

### Microsoft sign-in

Microsoft sign-in needs an Azure application registered with the
`XboxLive.signin` scope and approved by Mojang — each launcher must have its
own, so none is baked in. Supply yours at build time or at runtime:

```bash
JUSTLAUNCHER_MSA_CLIENT_ID=<your-client-id> pnpm tauri build
```

Without it, offline accounts still work (single player and LAN only).

## Layout

```
src/                Svelte UI
  lib/api.ts        typed wrapper over the Tauri commands
src-tauri/src/
  mojang.rs         version metadata model + OS rule engine
  install.rs        version resolution, downloads, natives extraction
  launch.rs         JVM command construction and process supervision
  auth.rs           Microsoft/Xbox/Minecraft token chain
  instance.rs       instance directories and config
  java.rs           JVM discovery and version matching
  jre.rs            downloading Mojang's Java runtimes
  download.rs       parallel fetcher with checksum verification
  loader.rs         Fabric and Quilt loader metadata
  settings.rs       launcher-wide preferences and new-instance defaults
  pack.rs           instance export/import archives, duplication
  mrpack.rs         Modrinth modpack (.mrpack) import
  mods.rs           mods, resourcepacks, shaderpacks: list, enable, delete
  modrinth.rs       Modrinth search and jar resolution
```

Data lives in `%APPDATA%/JustLauncher` (`~/.local/share/JustLauncher` on Linux),
overridable with `JUSTLAUNCHER_HOME`.

## Tests

```bash
pnpm check                                   # frontend typecheck
pnpm test                                    # frontend unit tests (node --test)

cd src-tauri
cargo test                                   # offline unit tests
cargo test --test live_metadata -- --ignored # hits Mojang/Fabric
cargo test --test live_jre -- --ignored      # hits Mojang's Java runtime index
```

`live_metadata` resolves real versions (modern, legacy 1.8.9, Fabric) and
asserts the built command line has no unexpanded `${...}` placeholders. Run it
after touching version resolution or argument building. `live_jre` asserts
Mojang still publishes every Java runtime component the launcher relies on.
