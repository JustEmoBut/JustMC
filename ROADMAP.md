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
- **Server list with live pings.** `servers.dat` is read and written through a
  small NBT module that round-trips keys the launcher does not understand, and
  each entry is pinged over the status protocol for its MOTD, player count,
  latency and icon. A bare host goes through an `_minecraft._tcp` SRV lookup
  (`hickory-resolver`) the way the game's own list does; a typed port is left
  alone.

## 1. Forge and NeoForge

The biggest loader gap, and the biggest job here: installer-profile parsing,
deploy steps and their extra libraries. Reference entry points:
`launcher/minecraft/update/` and the component system in
`launcher/minecraft/PackProfile.cpp`.

## 2. Quick Play: join a server or world on double click

`--quickPlayMultiplayer <address>` and `--quickPlaySingleplayer <folder>`
(1.20+) launch straight into a server or a save, which is what the Servers and
Worlds panels are missing. A few arguments in `launch::build_command` plus a
double-click handler in each panel. Prism shipped this on 2026-08-21 (#5941).

## 3. Total play time, and sorting by it

Per-instance play time is already recorded. What is missing is the launcher's
own total, sorting the instance list by it, and a per-instance opt out of
being counted. Prism: #5881, #5714, #5765.

## 4. Loose ends from Prism's develop

Small, each independent, each with a real complaint behind it:

- The device code flow reports nothing when authorisation fails — no message,
  no code, no retry. Prism #5645; `auth.rs` already translates XSTS `XErr`
  codes and this deserves the same.
- A JVM that fails to spawn surfaces in the UI but never reaches
  `logs/latest.log`, which is the file a user is asked to send. Prism #5644.
- `UpdateReview.svelte` has no select-all / select-none. Prism #5946.
- No restart button while an instance runs. Prism #5646.

## 5. Skin management

Upload and select skins and capes against
`api.minecraftservices.com/minecraft/profile`, and render avatars from the
profile's own skin instead of the third-party `api.mineatar.io`. Reference:
`launcher/minecraft/skins/`.

## 6. Launcher self-update

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

CurseForge (API key and download restrictions), world creation and renaming,
custom themes, Prism/MultiMC instance import, i18n.

Also declined from Prism's develop, deliberately: multiple instance
directories and choosing one per instance (#5827) — the launcher has one root
and the filesystem is the database; wildcards in the instance name field
(#5837); an icon-picker category selector (#4397).
