# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Minecraft launcher: Rust + Tauri 2 backend, Svelte 5 + Vite frontend. A
clean-slate rewrite of a Qt/C++ Prism Launcher fork; it does **not** read
Prism/MultiMC data. Scope is vanilla, Fabric, Quilt, Forge and NeoForge
launching, plus the content folders of an instance, installing mods from
Modrinth and importing a Modrinth modpack — see README for what is deliberately
out of scope.

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
args) and whether the window minimises while a game runs. Saving them never
touches an existing instance — that is what the instance dialog is for.

Missing or unreadable falls back to defaults rather than erroring, and the
struct is `#[serde(default)]`, so a file from an older or newer build still
loads. The default heap is half the machine's RAM clamped to 2–8 GB, not a
constant: a flat 4 GB is refused outright by `launch` on an 8 GB machine and is
needlessly timid on a large one.

**Window calls are permission-gated.** `minimize`, `unminimize` and `setFocus`
are *not* in `core:window:default` — only the read-only `is-minimized` kind is.
A missing permission rejects the promise rather than erroring visibly, so any
new `getCurrentWindow()` call needs both an entry in
`capabilities/default.json` and a `.catch` that reports.

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
- **Fabric and Quilt** are profiles that inherit from a vanilla version.
  `mojang::merge` stacks one; child libraries go **first** so a patched copy
  shadows vanilla's. Forge inherits the same way but only after its jar is
  built — see `forge.rs`.

### Loaders (`loader.rs`)

Fabric and Quilt publish the same shape of metadata and share one module. They
differ in three details, each verified against the live API and each a real bug
if assumed away:

- **"never supported" is a different status.** Fabric answers **400**, Quilt
  **404**. Both are answers ("no loaders"), not failures, so the UI can say so
  before an instance is created.
- **Fabric's `stable` flag marks the single build it recommends** — every other
  loader is flagged false, so never present those as "unstable". Quilt publishes
  no such flag and means it: it ships betas as its normal channel, so a Quilt
  instance takes the **newest** build and the picker labels nothing recommended.
  Deriving "stable" from the absence of `-beta` would pin a build months behind.
- **Quilt's list is unordered.** Fabric returns newest first; Quilt returned
  `0.20.0-beta.9, 0.20.0-beta.7, …, 0.24.0`, so `loaders` sorts both.

**Each project's own profile is the whole install.** The Qt original models
Fabric as *two* components against its own metadata server: the loader
(`net.fabricmc.fabric-loader`: Knot, asm, mixin) plus a `volatile`
`net.fabricmc.intermediary` pinned to the Minecraft version — and Quilt's
component `requires` Fabric's intermediary too. That split exists to serve its
component system, which resolves and re-pins pieces independently. Fetching
`/v2/versions/loader/<mc>/<loader>/profile/json` returns all of it already
merged, so there is nothing here to split and no metadata server to run.

Which Modrinth loader tags a mod search uses belongs to the **instance**, not
the folder: `Loader::mod_loaders` asks for `quilt` *and* `fabric` on a Quilt
instance, because Quilt runs Fabric mods and asking for `quilt` alone returned
4.6k of the 18.5k mods available for 1.21.1. Modrinth ORs the entries inside one
facet and ANDs the facets, so both tags must share one bracket pair.

### Forge and NeoForge (`forge.rs`)

The other loaders publish a profile and installing one is a download. Forge
ships an **installer**, and the client jar it wants does not exist anywhere
until this machine builds it. That is the whole difference, and why this is not
`loader.rs`.

An installer holds `version.json` (an ordinary `inheritsFrom` profile, so
`mojang::merge` stacks it like Fabric's), `install_profile.json` (the recipe:
extra libraries, a `data` map, and `processors` — tool jars to run with a JVM),
and sometimes a `maven/` tree of artifacts no repository serves.

Both projects use the same format, `spec` 1. Forge before 1.13 used a different
installer and is refused with a message rather than failing halfway through.

The work is split in two so the module never names `AppHandle`:

- `profile` fetches the installer and unpacks the two JSON files. No JVM, so it
  runs wherever a version is resolved.
- `tool_jobs` + `patch` build the jar. `install::install` runs the downloads
  through its own progress window and then calls `patch`, which shells out to
  the same JVM the game will use. Both are no-ops once the patched jar exists —
  its absence is the only reason to run a series of JVMs.

**A test binary that so much as names `AppHandle` will not start** (Windows:
`STATUS_ENTRYPOINT_NOT_FOUND`, the webview runtime gets linked into a console
exe). That is what the split is for, and why `download::run_quiet` exists
alongside `run`: it is the same parallel, verified download with no events.

Four details, each verified against a real installer (Forge 1.21.1-52.1.16,
NeoForge 21.1.248) and each a silent wrong result if assumed away:

- **A library with an empty `url` is an output, not a download.** Forge's
  `forge:<version>:client` is the patched jar: on the classpath, with a hash
  and a size, produced locally. `Library::resolve` returns no job for it.
- **NeoForge does not put its patched jar on the classpath at all.** FML finds
  it under `libraryDirectory` from the `--fml.*` arguments, so the only proof
  it was built is that the file exists.
- **Maven coordinates here carry an `@extension`** (`neoform:1.21.1-...@zip`,
  `...:mappings@txt`). Mojang's metadata never does; reading it as part of the
  version makes a directory nothing will ever create.
- **`data` values come in three shapes**: `[maven]` is a file in the library
  store, `/data/client.lzma` is a path from the *installer's root*, and
  `'quoted'` is a literal.

`live_metadata` installs both for real — the processors are a series of JVM
invocations against a recipe no unit test stands in for, and a wrong step
produces a jar that exists and crashes.

**Why not the Qt original's route.** It never runs the processors. Its
metadata server converts each installer into a component ahead of time, and
the profile's `mainClass` is not Forge's at all but **ForgeWrapper**, a
third-party shim that runs the processors *at launch* and then hands off. That
buys a launcher two dependencies: a metadata service to operate, and a pinned
wrapper build to keep current (`ForgeWrapper:prism-2026-08-01` at the time of
writing). Running the recipe at install time is what the official installer
does, needs neither, and puts nothing extra on the classpath. Its `mavenFiles`
— downloaded, deliberately off the classpath — is the same idea as the tool
jars `tool_jobs` fetches, and it hits the same `@zip` coordinates, which is
independent confirmation that both details are real rather than guessed.

Two things it carries that this launcher does not need: a conflict matrix
between loaders (an instance here has one `loader` field, so two cannot be
installed at once) and a legacy FML libraries step for Forge older than 1.6,
which is well below the 1.13 floor. Its fifth loader, LiteLoader, stopped at
1.12.2 in 2017 and is not worth a module.

### Instance archives (`pack.rs`, `mrpack.rs`)

Two zip formats, kept apart because only the extension distinguishes them:
`import_archive` in `lib.rs` is the single place that dispatches, so a picker,
a drop and any future caller all route the same way.

- `pack.rs` is **our own** format: the instance config plus its whole
  `.minecraft`. Libraries and assets are deliberately excluded — they live in
  the shared store and re-downloading is cheaper than shipping them. `duplicate`
  reuses the same `collect` filter as `export`, which is what keeps natives and
  logs out of a copy for free.
- `mrpack.rs` is **Modrinth's**: an index of URLs with hashes plus `overrides/`
  folders. It downloads rather than unpacks, so every file becomes a
  `download::Job` and is SHA-1 verified like any library. The index is
  attacker-controlled, so `safe_join` guards every path and plain-HTTP downloads
  are refused. A pack names its loader with a key of its own
  (`fabric-loader`, `quilt-loader`, `forge`, `neoforge`) and the more specific
  one wins when two are listed; `env.client == "unsupported"` marks a
  server-only file and is the only reason to skip one. A failure part-way
  deletes the instance rather than leaving something that looks playable.

Changing an instance's Minecraft version is `update_instance`'s job, not the
UI's: it clears `loader_version` and `installed` whenever `mc_version` moves, so
the next launch re-resolves both. A Fabric loader build is only listed for the
versions it supports, which is why a pinned one cannot survive the change.

### Content folders (`mods.rs`, `modrinth.rs`)

The folder is the database, like the instance list: `mods::list` reads
`.minecraft/<folder>` and nothing caches it. A file is disabled by appending
`.disabled` to its name — the convention every launcher shares, so the user's
other tools still understand the folder.

`mods/`, `resourcepacks/` and `shaderpacks/` are one implementation, not three.
`Kind` is the only thing that differs between them and carries all four
differences: the folder name, the extension the game reads (`.jar` vs `.zip`),
Modrinth's `project_type`, and the loaders a build is tagged with. Those last
two are **verified against the live API**, not guessed — a resource pack build
is `minecraft`, a shader is `iris` *and* `optifine` (Iris reads OptiFine
shaders), and a wrong value returns an empty catalogue rather than an error, so
`live_metadata` asserts real results come back. Only mods narrow the search by
loader, and only mods install dependencies: a pack's dependency is the loader
itself, which cannot go in a pack folder.

Names come from whatever the archive states: `fabric.mod.json` for a mod,
`pack.mcmeta` plus `pack.png` for a resource pack, and the file name when there
is neither — which is the usual case for a shader.

A pack does not have to be a zip. The game reads one unpacked into a directory
just as happily, so `list` lists those too — but only when the game itself
would: `pack.mcmeta` for a resource pack, a `shaders/` subdirectory for a
shader. That test is what keeps an unrelated folder someone parked in there off
a list whose trash button is a `remove_dir_all`; `ModFile.dir` marks the ones
that are, and the frontend confirms before deleting one. A folder has no jar to
hash, so it carries no SHA-1 and Modrinth never claims it — it cannot be
matched, updated or counted as installed, which is correct, since nothing on
Modrinth ships unpacked. Mods stay archives only: a loader ignores an exploded
jar.

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

### Screenshots (`screenshots.rs`)

The one instance folder a user wants to look at rather than manage, so the
images are served straight to the webview over Tauri's `asset:` protocol
instead of crossing IPC: a screenshot is a couple of megabytes and a grid of
them would be a hundred, base64-encoded, on every open.

That protocol is enabled in `tauri.conf.json` with an **empty** scope, and
`img-src` carries `asset: http://asset.localhost`. Nothing is readable until
`list` allows the folder it just read, because the folder sits under whatever
`JUSTLAUNCHER_HOME` points at and no static glob can describe that. The
frontend turns a listed path into a URL with `convertFileSrc`; deleting still
goes through `checked_name` and an extension check, like every other folder.

### Worlds (`worlds.rs`)

Narrow on purpose: list, back up, restore, delete. The game creates and renames
worlds better than a launcher can, but it has no backup button next to its
delete one — and `.minecraft/saves` is the only thing in an instance that
cannot be re-downloaded, which is the whole reason this exists.

A folder is a world when it holds a `level.dat`, and that file is also where
the in-game name and the last-played time come from — parsed with the `nbt`
module written for `servers.dat`. `LastPlayed` is what the game itself shows
and, unlike the file's modification time, survives a copy or a restore;
anything unreadable falls back to the folder name and the mtime.

Backups are `pack::zip_dir` — the same walk and filter `export` uses — writing
into `exports/` with the world folder as the entry prefix, so unpacking into
`saves` restores it. `world_dir` is the single chokepoint: it runs
`checked_name` and then insists on a `level.dat`, so neither a crafted name nor
a stray folder can be handed to `remove_dir_all`.

**The archive names the world, not the file.** `backups` lists `exports/` by
the shape `backup` writes (`<id>-<folder>-<unix>.zip`) — the file name is the
only index there is, which is what keeps exports/ a folder the user can tidy
by hand. `restore` then takes the folder out of the *zip's* entries instead,
so a renamed backup still works, and it checks everything before it deletes
anything: one root folder, holding a `level.dat`. The world is replaced rather
than merged into, because chunks from two saves in one folder is a corrupt
world.

### Options (`read_options`, `write_options` in `lib.rs`)

`.minecraft/options.txt` edited as text, in two commands too small for a
module. Text is what the file is — `key:value` per line — and a form would
have to know all ~150 keys the game keeps adding to. Refused while the game
runs, like worlds and the server list: the client rewrites the file wholesale
on exit. A missing file reads as empty rather than as an error, so an instance
that has never launched gets its first one from here.

### Servers (`servers.rs`, `nbt.rs`)

The game's own multiplayer list, `.minecraft/servers.dat`, edited in place —
which is why `nbt.rs` exists at all.

`nbt.rs` parses the whole tag tree and writes it back byte-for-byte, keeping
compound entries in file order. That is not thoroughness for its own sake: the
client writes keys this launcher has never heard of (`acceptTextures`, whatever
a version adds next) and rebuilding the file from the fields we understand
would drop them silently. Every read is bounds-checked and a declared length is
refused when it runs past the file, because a corrupt `.dat` must be an error
rather than a panic or a gigabyte allocation. `worlds.rs` reads `level.dat`
through the same module, which is where its world names and `LastPlayed` come
from.

**The list is not touched while the game runs.** The client rewrites
`servers.dat` wholesale on exit, so anything added underneath it is lost; the
UI disables editing exactly as it does for worlds. Writes go through a
`.dat.part` rename, since a half-written file is a list the player loses.
Removal sends the entry's address alongside its position — the file has no ids,
two entries may share a name, and a stale index would delete a neighbour.

`ping` speaks the status protocol directly (varints, handshake, one JSON
reply); no client library is involved. Three things it has to get right:

- **A bare host gets an SRV lookup, an explicit port never does.** Shared
  hosting sells one hostname behind one IP on a scattered port, and
  `_minecraft._tcp.<host>` is how the game finds it — without it a large slice
  of small servers looks offline. A player who typed `:25565` meant that port,
  so `split_address` reports whether the port was written down rather than
  comparing against the default. `hickory-resolver` does the lookup; failure of
  any kind falls back to the address as typed.
- **The MOTD arrives in three shapes**: a plain string, a chat component with
  `extra`, or either one full of `§` colour codes. It is flattened to text on
  the way out, because the list renders none of that.
- **The reply is attacker-controlled.** It is capped before it is read, and a
  favicon only survives if it is already a `data:image/` URI — it goes straight
  into an `<img>`.

Pinging is one command per server so the UI shows each answer as it lands; a
dead address costs the whole five second timeout and a list of ten pinged in
turn would take a minute.

### Logs (`logs.rs`)

Three directories hold something the user calls "the log", and `Source` is the
only thing that separates them: `<instance>/logs` (what the launcher captured,
truncated per launch), `.minecraft/logs` (the game's own, everything but
`latest.log` gzipped) and `.minecraft/crash-reports`. The frontend sends a
`source` plus a bare file name, never a path — `checked_name` and an extension
check guard the join, exactly like the mods folder. The directory is the
database here too: nothing is indexed.

Reading tails the last 2000 lines, the same cap the live view applies, because
rendering a whole session is what makes launcher log views crawl. Old logs
carry the session token just like the live one, so they are shown in the same
panel and leave through the same `redact` chokepoint.

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

`finish_login` is the single place the chain ends, for both a fresh login and
a refresh; `minecraft_token` is where the XSTS token is traded. It tries the
current first-party endpoint (`launcher/login`, `PC_LAUNCHER`) and falls back
to the older `authentication/login_with_xbox`. The fallback is there because
no test account exists in this repo to prove the new reply shape against a
live response — remove it once the new path has been seen working, not before.

**Tokens live in the OS keychain, not `accounts.json`.** A live refresh token
is a login. `auth::save_all` moves the two token fields into the keychain
(`keyring`, service `JustLauncher`, one entry per account id) and writes the
file without them; `load_all` puts them back. Both token fields are
`skip_serializing_if = "String::is_empty"`, which is what makes the file
tokenless rather than token-blank.

The keychain is never assumed: a portable install, a headless Linux box with
no session bus, a locked keyring. When it refuses, the tokens go back into the
file and the user stays signed in — the old weakness, now the exception. That
also means **a token found in the file wins on load**: it is the copy written
last, and it is how a pre-keychain `accounts.json` migrates (read once,
rewritten without them). Removing an account calls `auth::forget`, or the
credential outlives the account it belonged to.

## Conventions

### The IPC contract is the wire format

Tauri converts **command arguments** from camelCase to snake_case
(`mcVersion` → `mc_version`), but **serialized struct fields** keep their serde
names. `#[serde(rename = "type")]` renames in *both* directions, so a field read
as `type` is also *written* as `type`. `invoke<T>()` is an unchecked assertion —
a mismatch is silently `undefined` at runtime. When changing a Rust type that
crosses the boundary, update `src/lib/api.ts` in the same edit.

### A snippet must be a direct child of the component it is passed to

`{#snippet footer()}` reaches `Modal` as a prop only when it sits directly
inside `<Modal>…</Modal>`. Nested one level deeper — inside an `{#if}` that
switches the dialog's mode, say — Svelte treats it as a local snippet, passes
nothing, and the component renders no footer. Nothing warns: not
`svelte-check`, not the build. The Add Instance dialog lost its Cancel and
Create buttons that way, and the dialog still looked otherwise normal.

When a footer's content depends on state, put the condition *inside* the
snippet, never around it.

### Scroll containers

Setting `overflow-y: auto` makes the browser compute `overflow-x` as `auto`
too, so a few pixels of overhang draw a scrollbar across a whole dialog. Every
scrolling container in the app pairs it with `overflow-x: hidden`; content that
genuinely cannot wrap (code blocks, tables) scrolls inside its own box, and a
table needs `display: block` before it will.

### The CSP, and why `img-src` is wide

`default-src 'self'` governs everything the app loads; the directives that would
otherwise inherit it are pinned to `'none'` (objects, frames, workers, media,
`base-uri`, `form-action`) because the app uses none of them.

`img-src` is deliberately `https:` rather than an allowlist. The images the app
itself loads *are* a short list — `cdn.modrinth.com` for every project icon and
`api.mineatar.io` for avatars — but a Modrinth description is author-written
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

`src/lib/actions.ts` holds open-folder, export, duplicate, stop and delete.
The side panel, the settings dialog and the right-click menu (`ContextMenu.svelte`) all call it;
adding an action there is what makes it appear everywhere it belongs.

### Dependencies

Deliberately few. The frontend has three: `marked` and
`dompurify` (paired, for Modrinth descriptions) and `jsdom` for their test.
`zip`, `reqwest`, `tokio`, `serde` are already present — reach
for those before adding anything. Three crates here carry real weight, each for one job that is
not a few lines: `hickory-resolver` (SRV lookups need DNS packets *and* the
system's resolver configuration on three platforms), `keyring` (three
different OS credential stores) and `windows-sys` (already in the tree; one
call, `GlobalMemoryStatusEx`). The frontend has no UI framework beyond
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
