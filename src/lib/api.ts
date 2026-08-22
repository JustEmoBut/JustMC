import { invoke } from "@tauri-apps/api/core";

export type Loader = "vanilla" | "fabric" | "quilt";

/** Human label for a loader; "Vanilla" is what an unmodded instance is called. */
export function loaderName(loader: Loader) {
  return loader === "fabric" ? "Fabric" : loader === "quilt" ? "Quilt" : "Vanilla";
}

/** Which instance folder a mods call acts on; the values are the folder names. */
export type ModKind = "mods" | "resourcepacks" | "shaderpacks";

/** Launcher-wide preferences; the first three are defaults for new instances. */
export interface Settings {
  memory_mb: number;
  java_path: string;
  jvm_args: string;
  minimise_on_play: boolean;
}

export interface Instance {
  id: string;
  name: string;
  mc_version: string;
  loader: Loader;
  loader_version: string;
  memory_mb: number;
  java_path: string;
  jvm_args: string;
  last_played: number;
  /** Seconds the game has run in this instance, across every launch. */
  play_time: number;
  installed: boolean;
}

export interface ManifestVersion {
  id: string;
  /** "release" | "snapshot" | "old_beta" | "old_alpha", straight from Mojang. */
  type: string;
  url: string;
  releaseTime: string;
}

export interface VersionList {
  latest: { release: string; snapshot: string };
  versions: ManifestVersion[];
}

export interface Account {
  id: string;
  name: string;
  kind: "microsoft" | "offline";
}

export interface DeviceCode {
  user_code: string;
  device_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export interface JavaInstall {
  path: string;
  version: string;
  major: number;
  /** A 32-bit JVM cannot address more than ~2 GB of heap, whatever -Xmx says. */
  bit64: boolean;
}

export interface FabricLoader {
  version: string;
  stable: boolean;
}

export interface Screenshot {
  file: string;
  /** Absolute path; pass it through convertFileSrc to display it. */
  path: string;
  size: number;
  /** Unix seconds. */
  taken: number;
}

export interface World {
  /** The save directory name; every world command takes this. */
  folder: string;
  /** What level.dat calls it, falling back to the folder name. */
  name: string;
  size: number;
  /** Unix seconds, from level.dat's modification time. */
  last_played: number;
}

/** Which directory a log came from; also how the backend finds it again. */
export type LogSource = "launcher" | "game" | "crash";

export interface LogFile {
  file: string;
  source: LogSource;
  size: number;
  /** Unix seconds. */
  modified: number;
}

export interface ModFile {
  file: string;
  name: string;
  version: string;
  enabled: boolean;
  size: number;
  /** Icon from the jar's fabric.mod.json as a data: URI, when it ships one. */
  icon: string | null;
  /** A pack unpacked into a folder; deleting one removes a whole tree. */
  dir: boolean;
}

export interface ModHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
  follows: number;
  categories: string[];
  icon_url: string | null;
}

export interface ModSearchPage {
  hits: ModHit[];
  total_hits: number;
}

export interface ModProject {
  id: string;
  slug: string;
  title: string;
  description: string;
  /** Modrinth-flavoured markdown, rendered as plain text. */
  body: string;
  downloads: number;
  followers: number;
  categories: string[];
  icon_url: string | null;
  source_url: string | null;
  issues_url: string | null;
  wiki_url: string | null;
}

export interface ModVersion {
  id: string;
  name: string;
  version_number: string;
  /** "release", "beta" or "alpha". */
  version_type: string;
  date_published: string;
  downloads: number;
}

export interface ModUpdate {
  file: string;
  name: string;
  current_version: string;
  new_version: string;
  version_id: string;
  /** Release notes for the new build, markdown, often absent. */
  changelog: string | null;
}

export const api = {
  listVersions: () => invoke<VersionList>("list_versions"),
  listLoaders: (mcVersion: string, loader: Loader) =>
    invoke<FabricLoader[]>("list_loaders", { mcVersion, loader }),

  listInstances: () => invoke<Instance[]>("list_instances"),
  /** `loaderVersion` null means "resolve the loader build at install time". */
  createInstance: (
    name: string,
    mcVersion: string,
    loader: Loader,
    loaderVersion: string | null
  ) => invoke<Instance>("create_instance", { name, mcVersion, loader, loaderVersion }),
  updateInstance: (instance: Instance) => invoke<void>("update_instance", { instance }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
  openInstanceFolder: (id: string) => invoke<void>("open_instance_folder", { id }),
  exportInstance: (id: string) => invoke<string>("export_instance", { id }),
  importInstance: (path: string) => invoke<Instance>("import_instance", { path }),
  /** For a file the webview picked: it hands over content, never a path. */
  importArchiveBytes: (name: string, bytes: Uint8Array) =>
    invoke<Instance>("import_archive_bytes", { name, bytes }),
  duplicateInstance: (id: string, name: string) =>
    invoke<Instance>("duplicate_instance", { id, name }),
  openExportsFolder: () => invoke<void>("open_exports_folder"),
  installInstance: (id: string) => invoke<void>("install_instance", { id }),
  launchInstance: (id: string, accountId: string) =>
    invoke<void>("launch_instance", { id, accountId }),

  stopInstance: (id: string) => invoke<boolean>("stop_instance", { id }),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  beginMicrosoftLogin: () => invoke<DeviceCode>("begin_microsoft_login"),
  completeMicrosoftLogin: (code: DeviceCode) =>
    invoke<Account>("complete_microsoft_login", { code }),
  addOfflineAccount: (name: string) => invoke<Account>("add_offline_account", { name }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),

  listScreenshots: (id: string) => invoke<Screenshot[]>("list_screenshots", { id }),
  deleteScreenshot: (id: string, file: string) =>
    invoke<void>("delete_screenshot", { id, file }),

  listWorlds: (id: string) => invoke<World[]>("list_worlds", { id }),
  /** Zips the world into the exports folder; returns the archive path. */
  backupWorld: (id: string, folder: string) =>
    invoke<string>("backup_world", { id, folder }),
  deleteWorld: (id: string, folder: string) => invoke<void>("delete_world", { id, folder }),

  /** Past logs and crash reports of an instance, newest first. */
  listLogs: (id: string) => invoke<LogFile[]>("list_logs", { id }),
  /** The tail of one of them, decompressed when gzipped. */
  readLog: (id: string, source: LogSource, file: string) =>
    invoke<string[]>("read_log", { id, source, file }),

  openModsFolder: (id: string, kind: ModKind) =>
    invoke<void>("open_mods_folder", { id, kind }),
  listMods: (id: string, kind: ModKind) => invoke<ModFile[]>("list_mods", { id, kind }),
  setModEnabled: (id: string, kind: ModKind, file: string, enabled: boolean) =>
    invoke<string>("set_mod_enabled", { id, kind, file, enabled }),
  deleteMod: (id: string, kind: ModKind, file: string) =>
    invoke<void>("delete_mod", { id, kind, file }),
  searchMods: (
    query: string,
    mcVersion: string,
    sort: string,
    category: string | null,
    offset: number,
    kind: ModKind,
    loader: Loader
  ) =>
    invoke<ModSearchPage>("search_mods", {
      query,
      mcVersion,
      sort,
      category,
      offset,
      kind,
      loader,
    }),
  modProject: (id: string) => invoke<ModProject>("mod_project", { id }),
  modVersions: (project: string, mcVersion: string, kind: ModKind, loader: Loader) =>
    invoke<ModVersion[]>("mod_versions", { project, mcVersion, kind, loader }),
  /** Installs the file plus, for mods, its required dependencies; returns every file added. */
  installMod: (id: string, kind: ModKind, project: string, versionId: string | null) =>
    invoke<string[]>("install_mod", { id, kind, project, versionId }),
  /** Opens an http(s) link in the user's browser; other schemes are refused. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  addModFile: (id: string, kind: ModKind, path: string) =>
    invoke<string>("add_mod_file", { id, kind, path }),
  /** Modrinth project ids of what is already installed, matched by SHA-1. */
  installedModProjects: (id: string, kind: ModKind) =>
    invoke<string[]>("installed_mod_projects", { id, kind }),
  checkModUpdates: (id: string, kind: ModKind) =>
    invoke<ModUpdate[]>("check_mod_updates", { id, kind }),
  updateMod: (id: string, kind: ModKind, file: string, versionId: string) =>
    invoke<string>("update_mod", { id, kind, file, versionId }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  listJava: () => invoke<JavaInstall[]>("list_java"),
  /** Physical RAM in MB, or null when the platform will not say. */
  systemMemoryMb: () => invoke<number | null>("system_memory_mb"),
};

/** Tauri rejects with the backend's error string; normalise it for display. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
