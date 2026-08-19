import { api, errorMessage, type Instance } from "./api";
import { notify } from "./toast.svelte";

export async function openFolder(instance: Instance) {
  try {
    await api.openInstanceFolder(instance.id);
  } catch (e) {
    notify(errorMessage(e), "error");
  }
}

export async function openModsFolder(instance: Instance) {
  try {
    await api.openModsFolder(instance.id);
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
