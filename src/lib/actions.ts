import { api, errorMessage, type Imported, type Instance, type ModKind } from "./api";
import { notify } from "./toast.svelte";

export async function openFolder(instance: Instance) {
  try {
    await api.openInstanceFolder(instance.id);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

export async function openModsFolder(instance: Instance, kind: ModKind = "mods") {
  try {
    await api.openModsFolder(instance.id, kind);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

export async function duplicateInstance(instance: Instance) {
  try {
    const copy = await api.duplicateInstance(instance.id, `${instance.name} (copy)`);
    notify(`Created ${copy.name}.`);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

export async function stopInstance(instance: Instance) {
  try {
    if (!(await api.stopInstance(instance.id))) notify(`${instance.name} is not running.`);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

export async function deleteInstance(instance: Instance) {
  try {
    await api.deleteInstance(instance.id);
    notify(`Deleted ${instance.name}.`);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

/** Toast an import and open the page of every file the pack could not fetch. */
export function reportImport(imported: Imported) {
  if (imported.missing.length === 0) {
    notify(`Imported ${imported.name}.`);
    return;
  }
  // ponytail: opens one tab per file; a list dialog if packs block many
  const names = imported.missing.map((m) => m.file_name).join(", ");
  notify(
    `Imported ${imported.name}, but ${imported.missing.length} file(s) must be downloaded by hand into the matching folder: ${names}`,
    "error",
  );
  for (const m of imported.missing) api.openUrl(m.url).catch((e) => notify(errorMessage(e), "error"));
}
