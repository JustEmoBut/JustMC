import { api, errorMessage } from "./api";
import { notify } from "./toast.svelte";

/** The launcher's own update, shared by the top bar and the Settings page. */
export const updater = $state<{
  /** The newer release on offer: undefined until asked, null for "up to date". */
  update: { version: string; notes: string | null } | null | undefined;
  busy: boolean;
  /** The install confirmation `App.svelte` shows is open. */
  confirming: boolean;
}>({ update: undefined, busy: false, confirming: false });

/** `quiet` is the check run at startup: offline is normal there, not an error. */
export async function checkUpdate(quiet = false) {
  updater.busy = true;
  try {
    updater.update = await api.checkLauncherUpdate();
    if (!quiet && updater.update === null) notify("JustLauncher is up to date.");
  } catch (e) {
    if (quiet) console.warn("Update check failed:", e);
    else notify(errorMessage(e), "error");
  }
  updater.busy = false;
}

/** Installing restarts the launcher, so the buttons only ask first. */
export function askInstall() {
  updater.confirming = true;
}

export async function installUpdate() {
  updater.confirming = false;
  updater.busy = true;
  try {
    // Restarts the app on success, so nothing after this runs then.
    await api.installLauncherUpdate();
  } catch (e) {
    notify(errorMessage(e), "error");
    updater.busy = false;
  }
}
