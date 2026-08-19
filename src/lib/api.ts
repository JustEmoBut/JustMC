import { invoke } from "@tauri-apps/api/core";

export type Loader = "vanilla" | "fabric";

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

export interface ModFile {
  file: string;
  name: string;
  version: string;
  enabled: boolean;
  size: number;
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
  listFabricLoaders: (mcVersion: string) =>
    invoke<FabricLoader[]>("list_fabric_loaders", { mcVersion }),

  listInstances: () => invoke<Instance[]>("list_instances"),
  createInstance: (name: string, mcVersion: string, loader: Loader) =>
    invoke<Instance>("create_instance", { name, mcVersion, loader }),
  updateInstance: (instance: Instance) => invoke<void>("update_instance", { instance }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
  openInstanceFolder: (id: string) => invoke<void>("open_instance_folder", { id }),
  exportInstance: (id: string) => invoke<string>("export_instance", { id }),
  importInstance: (path: string) => invoke<Instance>("import_instance", { path }),
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

  openModsFolder: (id: string) => invoke<void>("open_mods_folder", { id }),
  listMods: (id: string) => invoke<ModFile[]>("list_mods", { id }),
  setModEnabled: (id: string, file: string, enabled: boolean) =>
    invoke<string>("set_mod_enabled", { id, file, enabled }),
  deleteMod: (id: string, file: string) => invoke<void>("delete_mod", { id, file }),
  searchMods: (
    query: string,
    mcVersion: string,
    sort: string,
    category: string | null,
    offset: number
  ) => invoke<ModSearchPage>("search_mods", { query, mcVersion, sort, category, offset }),
  modProject: (id: string) => invoke<ModProject>("mod_project", { id }),
  modVersions: (project: string, mcVersion: string) =>
    invoke<ModVersion[]>("mod_versions", { project, mcVersion }),
  /** Installs the jar plus its required dependencies; returns every file added. */
  installMod: (id: string, project: string, versionId: string | null) =>
    invoke<string[]>("install_mod", { id, project, versionId }),
  /** Opens an http(s) link in the user's browser; other schemes are refused. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  addModFile: (id: string, path: string) => invoke<string>("add_mod_file", { id, path }),
  /** Modrinth project ids of the jars already installed, matched by SHA-1. */
  installedModProjects: (id: string) => invoke<string[]>("installed_mod_projects", { id }),
  checkModUpdates: (id: string) => invoke<ModUpdate[]>("check_mod_updates", { id }),
  updateMod: (id: string, file: string, versionId: string) =>
    invoke<string>("update_mod", { id, file, versionId }),

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
