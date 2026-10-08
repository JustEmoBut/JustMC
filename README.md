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
- Fabric, Quilt, Forge and NeoForge loaders, with the loader build pinnable
  when an instance is created and afterwards. Forge and NeoForge run their own
  installer's recipe to build the patched client jar, so no external installer
  is involved; Forge older than 1.13 uses a different format and is refused
  with a message
- Create, configure, delete instances; per-instance memory, Java and JVM args
- Per-instance commands run before the game starts and after it exits, with
  the instance's folders in the environment; a failed one cancels the launch
- Parallel downloading with SHA-1 verification, resumable across runs,
  retried with backoff on a blip or a rate limit, with live transfer rate
  and ETA
- Shared library/asset store, so instances on the same version cost no extra disk
- Every asset layout Minecraft has used, including the named-file trees that
  1.7.2 and older need
- Mod management for Fabric instances, and resource packs and shaders for any
  instance: list, filter by name or file name, enable, disable, delete, drop a
  file on the window, and check what is installed for newer builds; a pack
  unpacked into a folder is listed like a zip
- Browse Modrinth from inside the launcher — mods, resource packs and shaders:
  sort by relevance, downloads, followers or date, filter by category, hide what
  is already installed, read a project and install any version of it, with its
  required dependencies
- Browse Modrinth's modpacks from the Add Instance dialog and install any build
  of one, whichever loader it asks for
- Export an instance as a Modrinth `.mrpack` any launcher can install — jars
  Modrinth can serve become downloads, the rest overrides, and the player's
  own worlds, screenshots, keybinds and server list stay out
- Export an instance to a zip; import one, a `.mrpack` or a CurseForge modpack
  zip, from Add Instance or by dropping it on the window
- Browse CurseForge beside Modrinth — mods, resource packs and shaders — and
  install with required dependencies; CurseForge and its modpacks need your own
  API key in Settings, and files whose authors forbid third-party downloads are
  not fetched, their pages are opened instead
- The installed list tags each file Modrinth, CurseForge or Local, by asking
  both catalogues which ones they recognise
- Duplicate an instance with its worlds and configs; change an existing
  instance's Minecraft version
- Java detection matched to each version's required major, and automatic
  download of Mojang's own runtime when the machine has none
- Total play time per instance, and launcher settings: the memory, Java and JVM
  arguments a new instance starts with, and whether the window closes while
  the game runs
- Live game log with error highlighting, plus the game's own old logs and
  crash reports; copying redacts session tokens and usernames
- Per-instance worlds with their in-game names: back one up as a zip that
  unpacks straight back into `saves`, or delete it
- Restore a world from any backup taken of it, replacing the save rather than
  merging into it
- Launch with no connection: everything the launch path reads is cached, and
  the version manifest falls back to its last copy
- A per-instance window size, and a sweep that reports and removes the shared
  versions, assets and Java runtimes no instance needs any more
- Edit the game's own `options.txt` from the launcher, without starting the game
- Browse an instance's screenshots and delete the ones not worth keeping
- The instance's multiplayer server list, read from and written to the game's
  own `servers.dat`, with a live status ping per server: MOTD, player count,
  latency and icon, SRV records resolved like the game does
- Microsoft sign-in tokens kept in the OS keychain rather than in a file, with
  a fallback for setups that have none
- Skins and capes for a Microsoft account: upload a skin with classic or slim
  arms, reset it, wear or hide a cape; avatars are cut from the account's own
  skin
- An account per instance, falling back to the selected one when it is gone,
  and per-instance environment variables for the game and its commands
- A world's data packs: add, enable, disable and delete them
- A desktop shortcut per instance that starts the launcher straight into it
- Modpack updates: an instance installed from a Modrinth pack is offered the
  pack's newest build, which replaces the pack's files and keeps the player's
- Launcher self-update from GitHub Releases, verified with a signing key

## Not included

World creation and renaming, custom themes, update checks for files only
CurseForge knows, updates for CurseForge modpacks. Planned work lives in the
[roadmap](ROADMAP.md).

CurseForge is the second catalogue, not the first. Its API needs a
per-launcher key, which no build of this launcher ships: the user supplies their
own Core API key (console.curseforge.com) in Settings, and without one the
CurseForge tab stays disabled. Its terms forbid caching what the API returns,
and it lets authors forbid third-party downloads, which turns "install" into
"open a browser and do it yourself" for a slice of the catalogue. Modrinth needs
no key and allows all of it, so it stays the default.

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

### Launcher updates

Releases are signed with a [minisign key](https://v2.tauri.app/plugin/updater/#signing-updates);
the public half is `plugins.updater.pubkey` in `tauri.conf.json`, so every
build verifies what it installs. `.github/workflows/release.yml` does the release: bump
`version` in `tauri.conf.json` and `Cargo.toml`, then push a matching tag.

```bash
git tag v0.2.0
git push origin v0.2.0
```

It builds the Windows (NSIS) installer, signs it, and attaches it with
`latest.json` to a **draft** release; publishing the draft is what makes it the
update installs see (`releases/latest/download/latest.json`). It reads the
repository secrets `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
and, optionally, `JUSTLAUNCHER_MSA_CLIENT_ID`. The
key pair came from `pnpm tauri signer generate`; losing the private key or its
password means no later release can update an existing install.

A signed build by hand needs the two signing variables and
`pnpm tauri build --config src-tauri/tauri.release.conf.json`.

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
  mrpack.rs         Modrinth modpack (.mrpack) import and export
  curseforge.rs     CurseForge browsing, fingerprints and modpack import
  mods.rs           mods, resourcepacks, shaderpacks: list, enable, delete
  modrinth.rs       Modrinth search and jar resolution
  forge.rs          Forge/NeoForge installer profiles and jar patching
  worlds.rs         world listing, backup and delete
  servers.rs        servers.dat editing and status pings
  nbt.rs            the slice of NBT servers.dat needs
  logs.rs           game, launcher and crash report logs
  screenshots.rs    screenshot listing and delete
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
