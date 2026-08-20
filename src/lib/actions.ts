import { api, errorMessage, type Instance, type ModKind } from "./api";
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

export async function exportInstance(instance: Instance) {
  try {
    await api.exportInstance(instance.id);
    notify(`Exported ${instance.name}.`);
    await api.openExportsFolder();
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
