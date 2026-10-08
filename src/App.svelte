<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import * as actions from "./lib/actions";
  import {
    api,
    errorMessage,
    type Account,
    type Instance,
    type ModKind,
    type QuickPlay,
    type Settings as SettingsData,
  } from "./lib/api";
  import ContextMenu, { type MenuItem } from "./lib/ContextMenu.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import { appendLine, clearLog } from "./lib/gamelog.svelte";
  import Icon from "./lib/Icon.svelte";
  import { instanceMenu } from "./lib/instanceMenu";
  import InstancePanel from "./lib/InstancePanel.svelte";
  import InstanceTile from "./lib/InstanceTile.svelte";
  import InstanceWindow from "./lib/InstanceWindow.svelte";
  import Modal from "./lib/Modal.svelte";
  import NewInstance from "./lib/NewInstance.svelte";
  import SettingsWindow from "./lib/SettingsWindow.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import TaskWindow from "./lib/TaskWindow.svelte";
  import { task, type Progress } from "./lib/task.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import { notify } from "./lib/toast.svelte";
  import TopBar from "./lib/TopBar.svelte";

  let instances = $state<Instance[]>([]);
  let accounts = $state<Account[]>([]);
  let selectedAccount = $state("");
  let selectedId = $state("");
  let search = $state("");
  /** How the grid is ordered; "recent" is what the backend already hands over. */
  let sort = $state<"recent" | "name" | "played">("recent");

  /** Launcher-wide preferences, loaded once; the settings window hands back what it saved. */
  let settings = $state<SettingsData | null>(null);

  let showNew = $state(false);
  /** The open page of the settings window, or "" while it is closed. */
  let settingsPage = $state("");
  /** The instance whose window is open ("" for none), and the page it shows. */
  let windowId = $state("");
  let windowPage = $state("log");
  /** Bumped when a dropped file lands, to make the content page re-read the folder. */
  let modsChanged = $state(0);

  /** Instance ids currently installing or running, with the label to show. */
  let busy = $state<Record<string, string>>({});
  let dragging = $state(false);
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  let deleting = $state<Instance | null>(null);
  let exporting = $state<Instance | null>(null);
  /**
   * The instance to launch again as soon as its process reports it has gone.
   * Restart cannot wait on `stop`: the kill is asynchronous and a second launch
   * before the first exits is refused, so the exit event is the signal.
   */
  let restarting = $state("");

  const visible = $derived(
    instances
      .filter((i) =>
        `${i.name} ${i.mc_version}`.toLowerCase().includes(search.trim().toLowerCase())
      )
      .sort((a, b) => {
        if (sort === "name") return a.name.localeCompare(b.name);
        if (sort === "played") return b.play_time - a.play_time;
        return 0; // the backend already sorted by last played
      })
  );

  const selected = $derived(visible.find((i) => i.id === selectedId));
  const activeAccount = $derived(accounts.find((a) => a.id === selectedAccount));
  /** Looked up by id so a refresh hands the window the instance's new state. */
  const windowed = $derived(instances.find((i) => i.id === windowId));

  // Keep a selection alive: the right column is the point of this layout, and
  // an empty one after every filter change would make the window feel broken.
  $effect(() => {
    if (visible.length && !visible.some((i) => i.id === selectedId)) {
      selectedId = visible[0].id;
    }
  });

  async function refreshInstances() {
    instances = await api.listInstances();
  }

  async function refreshAccounts() {
    accounts = await api.listAccounts();
    if (!accounts.some((a) => a.id === selectedAccount)) {
      selectedAccount = accounts[0]?.id ?? "";
    }
  }

  function openWindow(instance: Instance, page: string) {
    windowId = instance.id;
    windowPage = page;
  }

  $effect(() => {
    refreshInstances();
    refreshAccounts();
    api.getSettings().then((s) => (settings = s));

    const unlisten = [
      // One subscription for the whole app: whoever started the download owns
      // the label, the window renders whatever is running.
      listen<Progress>("install-progress", (e) => task.report(e.payload)),
      listen<{ line: string }>("game-log", (e) => appendLine(e.payload.line)),
      listen<{ instance: string; code: number }>("game-exited", (e) => {
        delete busy[e.payload.instance];
        const instance = instances.find((i) => i.id === e.payload.instance);
        const name = instance?.name ?? "Game";
        if (restarting === e.payload.instance) {
          restarting = "";
          if (instance) play(instance);
          refreshInstances();
          return;
        }
        if (e.payload.code === 0) {
          notify(`${name} closed.`);
        } else {
          notify(`${name} exited with code ${e.payload.code}. Check the log.`, "error");
          if (instance) openWindow(instance, "log");
        }
        refreshInstances();
      }),
    ];

    // Dropping an exported .zip imports it.
    const dropped = getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type === "over") {
        dragging = true;
      } else if (event.payload.type === "leave") {
        dragging = false;
      } else if (event.payload.type === "drop") {
        dragging = false;
        await importDropped(event.payload.paths);
      }
    });

    return () => {
      unlisten.forEach((p) => p.then((off) => off()));
      dropped.then((off) => off());
    };
  });

  const CONTENT_KINDS: ModKind[] = ["mods", "resourcepacks", "shaderpacks"];

  async function importDropped(paths: string[]) {
    // While a content page is open, a dropped file belongs to the folder it
    // is showing — a jar for mods, a zip for resource packs and shaders.
    const kind = CONTENT_KINDS.find((k) => k === windowPage);
    const extension = kind === "mods" ? ".jar" : ".zip";
    const content = paths.filter((p) => p.toLowerCase().endsWith(extension));
    if (windowed && kind && content.length) {
      for (const path of content) {
        try {
          notify(`Added ${await api.addModFile(windowed.id, kind, path)}.`);
        } catch (e) {
          notify(errorMessage(e), "error");
        }
      }
      modsChanged++;
      return;
    }

    const packs = paths.filter((p) => /\.(zip|mrpack)$/i.test(p));
    if (packs.length === 0) {
      notify("Drop an exported instance .zip, a CurseForge .zip or a Modrinth .mrpack to import it.", "error");
      return;
    }
    for (const path of packs) {
      // A pack downloads its whole mod list, so it needs the progress window.
      // A .zip may be a CurseForge pack, which only the backend can tell.
      task.begin("Importing pack");
      try {
        const imported = await api.importInstance(path);
        actions.reportImport(imported);
        selectedId = imported.id;
      } catch (e) {
        notify(errorMessage(e), "error");
      } finally {
        task.end();
      }
    }
    await refreshInstances();
  }

  /** Stop the game and start it again once the process is actually gone. */
  async function restart(instance: Instance) {
    busy[instance.id] = "Restarting…";
    // Nothing listening means the process is already gone and no exit event is
    // coming, so this is the only chance to start the next one.
    if (await api.stopInstance(instance.id)) {
      restarting = instance.id;
    } else {
      delete busy[instance.id];
      play(instance);
    }
  }

  async function play(instance: Instance, quickPlay: QuickPlay | null = null) {
    // An instance can name its own account; the backend falls back to the
    // selection when that one is gone, so only "neither exists" stops here.
    if (!selectedAccount && !accounts.some((a) => a.id === instance.account_id)) {
      settingsPage = "accounts";
      notify("Add an account first.", "error");
      return;
    }
    busy[instance.id] = instance.installed ? "Starting…" : "Installing…";
    clearLog();
    task.begin(instance.installed ? `Starting ${instance.name}` : `Installing ${instance.name}`);
    try {
      await api.launchInstance(instance.id, selectedAccount, quickPlay);
      busy[instance.id] = "Running";
      // Prism shows the console on launch; here that is the window's log page.
      openWindow(instance, "log");
      await refreshInstances();
    } catch (e) {
      delete busy[instance.id];
      restarting = "";
      notify(errorMessage(e), "error");
    } finally {
      task.end();
    }
  }

  function openMenu(instance: Instance, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation(); // otherwise the grid's background menu replaces this one
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: instanceMenu(instance, busy[instance.id], {
        play,
        restart,
        open: openWindow,
        exportInstance: (i) => (exporting = i),
        remove: (i) => (deleting = i),
        changed: refreshInstances,
      }),
    };
  }

  function openExports() {
    api.openExportsFolder().catch((e) => notify(errorMessage(e), "error"));
  }

  /** Right-click on empty space: the toolbar actions, where the cursor is. */
  function openBackgroundMenu(event: MouseEvent) {
    event.preventDefault();
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: "Add Instance", icon: "plus", action: () => (showNew = true) },
        { label: "Exports Folder", icon: "folder", action: openExports },
        { label: "Settings", icon: "gear", action: () => (settingsPage = "launcher") },
      ],
    };
  }

  async function confirmDelete() {
    if (!deleting) return;
    await actions.deleteInstance(deleting);
    if (deleting.id === windowId) windowId = "";
    deleting = null;
    await refreshInstances();
  }
</script>

<div class="app">
  <TopBar
    bind:search
    account={activeAccount}
    onnew={() => (showNew = true)}
    onfolders={openExports}
    onsettings={() => (settingsPage = "launcher")}
    onaccounts={() => (settingsPage = "accounts")}
  />

  <div class="body">
    <main class="library" oncontextmenu={openBackgroundMenu}>
      <div class="list-head">
        <h1>Instances</h1>
        <span class="count data">{instances.length}</span>
        <span class="spacer"></span>
        <select class="sort" bind:value={sort} aria-label="Sort instances">
          <option value="recent">Recent</option>
          <option value="name">Name</option>
          <option value="played">Play time</option>
        </select>
      </div>

      {#if visible.length === 0}
        <div class="empty">
          <h2>{search ? "No matches" : "No instances yet"}</h2>
          <p class="muted">
            {search
              ? "Nothing here matches that search."
              : "Create one, or drop a .zip or .mrpack anywhere on this window."}
          </p>
          {#if !search}
            <button class="primary" onclick={() => (showNew = true)}>
              <Icon name="plus" size={14} />
              Add Instance
            </button>
          {/if}
        </div>
      {:else}
        <div class="grid" role="listbox" aria-label="Instances" tabindex="-1">
          {#each visible as instance (instance.id)}
            <InstanceTile
              {instance}
              selected={instance.id === selectedId}
              running={!!busy[instance.id]}
              onselect={(i) => (selectedId = i.id)}
              onlaunch={play}
              onmenu={openMenu}
            />
          {/each}
        </div>
      {/if}
    </main>

    {#if selected}
      <aside class="panel">
        <InstancePanel
          instance={selected}
          status={busy[selected.id]}
          onlaunch={play}
          onrestart={restart}
          onopen={openWindow}
          onexport={(i) => (exporting = i)}
          onchanged={refreshInstances}
        />
      </aside>
    {/if}
  </div>

  <StatusBar {selected} {instances} />

  <TaskWindow />

  {#if menu}
    <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
  {/if}
</div>

{#if windowed}
  <InstanceWindow
    instance={windowed}
    status={busy[windowed.id]}
    bind:page={windowPage}
    changed={modsChanged}
    onclose={() => (windowId = "")}
    onplay={play}
    onsaved={refreshInstances}
    onexport={(i) => (exporting = i)}
  />
{/if}

{#if settingsPage && settings}
  <SettingsWindow
    {settings}
    {accounts}
    bind:page={settingsPage}
    onclose={() => (settingsPage = "")}
    onsaved={(saved) => (settings = saved)}
    onaccounts={refreshAccounts}
  />
{/if}

{#if showNew}
  <NewInstance onclose={() => (showNew = false)} oncreated={refreshInstances} />
{/if}

{#if deleting}
  <Modal title="Delete {deleting.name}" width="380px" onclose={() => (deleting = null)}>
    <p>Delete this instance and every world in it?</p>
    {#snippet footer()}
      <button onclick={() => (deleting = null)}>Cancel</button>
      <button class="danger" onclick={confirmDelete}>Delete</button>
    {/snippet}
  </Modal>
{/if}

{#if exporting}
  <ExportDialog instance={exporting} onclose={() => (exporting = null)} />
{/if}

{#if dragging}
  <div class="dropzone">
    <div class="dropzone-inner">
      <strong>Drop to import</strong>
      <span class="muted">An exported instance .zip, or a Modrinth .mrpack</span>
    </div>
  </div>
{/if}

<Toasts />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg);
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .library {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 24px 28px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .list-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .list-head h1 {
    font-size: 20px;
  }

  .count {
    color: var(--text-faint);
  }

  .sort {
    width: auto;
    padding: 5px 8px;
    font-size: 12.5px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(176px, 1fr));
    gap: 12px;
  }

  .panel {
    display: flex;
    width: 260px;
    flex: none;
    background: var(--bg-raised);
    border-left: 1px solid var(--border);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
    padding: 0 30px;
  }

  .empty h2 {
    font-size: 18px;
  }

  .empty p {
    margin: 0 0 8px;
    max-width: 340px;
    line-height: 1.5;
  }

  .empty button {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dropzone {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.6);
    backdrop-filter: blur(4px);
    pointer-events: none;
  }

  .dropzone-inner {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    padding: 38px 60px;
    border: 2px dashed var(--accent);
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
  }

  .dropzone-inner strong {
    font-size: 16px;
  }

  @media (max-width: 960px) {
    .panel {
      width: 220px;
    }
  }
</style>
