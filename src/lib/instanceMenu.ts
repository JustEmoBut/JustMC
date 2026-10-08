import * as actions from "./actions";
import type { Instance } from "./api";
import type { MenuItem } from "./ContextMenu.svelte";

export interface MenuHandlers {
  play: (instance: Instance) => void;
  restart: (instance: Instance) => void;
  /** Open the instance window on one of its pages. */
  open: (instance: Instance, page: string) => void;
  exportInstance: (instance: Instance) => void;
  remove: (instance: Instance) => void;
  changed: () => Promise<void>;
}

/** The right-click menu of an instance tile: the side panel's actions, in its order. */
export function instanceMenu(instance: Instance, status: string | undefined, on: MenuHandlers): MenuItem[] {
  const busy = !!status;
  return [
    { label: status ?? "Launch", icon: "play", disabled: busy, action: () => on.play(instance) },
    ...(status === "Running"
      ? [
          { label: "Kill", icon: "stop" as const, action: () => actions.stopInstance(instance) },
          { label: "Restart", icon: "refresh" as const, action: () => on.restart(instance) },
        ]
      : []),
    { label: "Edit", icon: "sliders", action: () => on.open(instance, "settings") },
    ...(instance.loader !== "vanilla"
      ? [{ label: "Mods", icon: "puzzle" as const, action: () => on.open(instance, "mods") }]
      : []),
    { label: "Worlds", icon: "globe", action: () => on.open(instance, "worlds") },
    { label: "Servers", icon: "server", action: () => on.open(instance, "servers") },
    { label: "Options", icon: "gear", action: () => on.open(instance, "options") },
    { label: "Screenshots", icon: "image", action: () => on.open(instance, "screenshots") },
    { label: "Log", icon: "terminal", action: () => on.open(instance, "log") },
    { label: "Folder", icon: "folder", action: () => actions.openFolder(instance) },
    ...(instance.loader !== "vanilla"
      ? [{ label: "Mods Folder", icon: "folder" as const, action: () => actions.openModsFolder(instance, "mods") }]
      : []),
    { label: "Export", icon: "export", action: () => on.exportInstance(instance) },
    { label: "Desktop Shortcut", icon: "play", action: () => actions.createShortcut(instance) },
    {
      label: "Copy",
      icon: "copy",
      disabled: busy,
      action: async () => {
        await actions.duplicateInstance(instance);
        await on.changed();
      },
    },
    { label: "Delete", icon: "trash", danger: true, disabled: busy, action: () => on.remove(instance) },
  ];
}
