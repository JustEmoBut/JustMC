# Roadmap

Ordered by effort-to-impact, from a feature comparison with the Qt/C++
[original](https://github.com/JustEmoBut/JustLauncher). Paths like
`launcher/net/...` refer to that repository, for studying the approach —
the code there is C++/Qt and gets rethought for this stack, not ported.

## Landed

- **Download retries and backoff.** Every file fetch gets three attempts with
  exponential waits (2 s, then 8 s), a 429's `Retry-After` is honoured, and the
  same ladder covers metadata requests. Previously one failed request aborted
  the whole batch.
- **Modrinth modpack browsing and `.mrpack` export.** The Add Instance dialog
  searches and installs packs, any build of them; Export offers the `.mrpack`
  format alongside the zip, with Modrinth-servable jars as downloads and the
  player's own files left out.
- **Account tokens in the OS keychain.** `accounts.json` keeps who the
  accounts are; `keyring` keeps what proves it, one entry per account. A
  keychain that refuses — portable install, no session bus — falls back to the
  file, and an old plaintext file migrates the first time it is read.
- **The current Minecraft token endpoint.** `launcher/login` with
  `PC_LAUNCHER`, with `authentication/login_with_xbox` as the fallback until
  the new path is seen working against a real account.
- **Forge and NeoForge.** `forge.rs` fetches an installer, unpacks its profile
  and runs the processors that build the patched client jar. Modern format
  only (`spec` 1: Forge 1.13+, every NeoForge); older Forge is refused with a
  message. `.mrpack` imports of Forge packs work as a result.
- **Quick Play.** A row in the Servers or Worlds panel launches straight into
  that server or save, by button or double click. Whether the version can be
  told is asked of its own metadata — 1.20+ declares the flags under
  `is_quick_play_*` feature rules — rather than by comparing version numbers,
  which would guess wrong on the snapshots either side of 1.20.
- **Play time, counted and sorted.** The status strip carries the launcher's
  own total, the instance grid sorts by name or play time as well as by when it
  was last played, and an instance kept for testing can opt out of being
  counted (`count_play_time`).
- **Four loose ends from Prism's develop.** A failed device-code sign-in says
  what went wrong instead of printing Microsoft's error slug (#5645); the
  command line and any spawn failure reach `logs/latest.log`, the file users
  are asked to send (#5644); the update review has select-all and select-none
  (#5946); and a running instance has a Restart button beside Stop (#5646).
- **Server list with live pings.** `servers.dat` is read and written through a
  small NBT module that round-trips keys the launcher does not understand, and
  each entry is pinged over the status protocol for its MOTD, player count,
  latency and icon. A bare host goes through an `_minecraft._tcp` SRV lookup
  (`hickory-resolver`) the way the game's own list does; a typed port is left
  alone.

- **Pre-launch and post-exit commands.** Two per-instance shell lines, run
  through `cmd /C` or `sh -c` in the game folder with the instance in the
  environment (`INST_DIR`, `INST_MC_DIR`, `INST_ID`, `INST_NAME`,
  `INST_MC_VERSION`). A failed pre-launch command cancels the launch; both
  commands' output lands in `logs/latest.log`. No wrapper command: the JVM is
  launched directly and nothing has needed to sit in front of it.

- **World restore, and the game's options.txt.** A world backup can be
  unpacked back over the save it was taken from — the archive names the folder,
  not the file name, and the world is replaced rather than merged into. The
  Options panel edits `.minecraft/options.txt` as text, which is what the file
  is; it is refused while the game runs, since the client rewrites it on exit.

- **Offline launching, a window size, and a shared-store sweep.** The version
  manifest and the Fabric/Quilt profile were the last two requests on the
  launch path with no cache; both have one now, so an installed instance starts
  with no connection. An instance can name the window size the game opens at
  (`--width`/`--height`, which the metadata gates behind a feature rule the
  launcher has to supply itself). `cleanup.rs` reports and removes the
  versions, asset generations and Java runtimes nothing points at any more —
  `libraries/` deliberately excluded, since a Forge output there is not
  distinguishable from an unused download.

## 1. Skin management

Upload and select skins and capes against
`api.minecraftservices.com/minecraft/profile`, and render avatars from the
profile's own skin instead of the third-party `api.mineatar.io`. Reference:
`launcher/minecraft/skins/`.

## 2. Launcher self-update

Check GitHub Releases, offer the update, swap the binary. The original ships a
standalone updater exe (`launcher/updater/`); a Tauri updater plugin likely
fits this stack better.

## Tech debt

Marked `ponytail:` in the code. Nothing is outstanding: the last of it was
cleared on 2026-08-22.

- ~~`java.rs` — RAM detected by shelling out~~ — `GlobalMemoryStatusEx` on
  Windows, so the launch path no longer spawns a process
- ~~`mods.rs` — jar metadata cache grows without a bound~~ — dropped whole at
  2000 entries
- ~~`worlds.rs` — world names found by scanning `level.dat` bytes~~ — parsed
  with the `nbt` module written for `servers.dat`, which also yields the game's
  own `LastPlayed` instead of the file's mtime
- ~~`servers.rs` — the round trip stands in for a ping packet~~ — the
  protocol's own ping is timed, with the round trip as the fallback
- `download.rs` — resumed files accepted on a size match, no re-hash. Kept, not
  deferred: hashing costs ~500 MB of reads every launch to catch corruption
  that happened after an already-verified download.

## Not planned

CurseForge update checks (browsing, installing and pack import exist; updates
still go through Modrinth), world creation and renaming,
custom themes, Prism/MultiMC instance import, i18n.

LiteLoader, the original's fifth loader: its last release was 1.12.2 in 2017.
Swappable LWJGL and Java-runtime vendors (the original serves Azul, IBM,
Adoptium and Mojang as components) are also out — Mojang's own runtime is what
`jre.rs` fetches, and nothing has needed replacing.

Also declined from Prism's develop, deliberately: multiple instance
directories and choosing one per instance (#5827) — the launcher has one root
and the filesystem is the database; wildcards in the instance name field
(#5837); an icon-picker category selector (#4397).
