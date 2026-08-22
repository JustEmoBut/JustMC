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

## 2. Encrypt stored account tokens

`accounts.json` holds live refresh and access tokens in plaintext — the same
weakness the Qt original has. Store them in the OS keychain (Tauri plugin or
equivalent), with a plaintext fallback for portable setups.

## 3. Migrate the Minecraft token endpoint

`auth.rs` takes the Minecraft token via `authentication/login_with_xbox`; the
original uses `api.minecraftservices.com/launcher/login` (`PC_LAUNCHER`
platform), the current first-party flow. Worth migrating before the old
endpoint ages out.

## 4. Skin management

Upload and select skins and capes against
`api.minecraftservices.com/minecraft/profile`, and render avatars from the
profile's own skin instead of the third-party `api.mineatar.io`. Reference:
`launcher/minecraft/skins/`.

## 5. Launcher self-update

Check GitHub Releases, offer the update, swap the binary. The original ships a
standalone updater exe (`launcher/updater/`); a Tauri updater plugin likely
fits this stack better.

## Tech debt

Marked `ponytail:` in the code:

- `download.rs` — resumed files accepted on size match alone, no re-hash
- `servers.rs` — the round trip stands in for the protocol's own ping packet
- `java.rs` — RAM detected by shelling out instead of a sysinfo call
- `mods.rs` — jar metadata cache grows without a bound
- `worlds.rs` — world names found by scanning `level.dat` bytes, not parsing NBT

## Not planned

CurseForge (API key and download restrictions), world creation and renaming,
custom themes, Prism/MultiMC instance import, i18n.
