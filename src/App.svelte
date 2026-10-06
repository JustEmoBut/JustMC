<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import * as actions from "./lib/actions";
  import {
    api,
    errorMessage,
    loaderName,
    type Account,
    type Instance,
    type LogFile,
    type ModKind,
    type QuickPlay,
    type Settings as SettingsData,
  } from "./lib/api";
  import Accounts from "./lib/Accounts.svelte";
  import ContextMenu, { type MenuItem } from "./lib/ContextMenu.svelte";
  import Icon from "./lib/Icon.svelte";
  import InstancePanel from "./lib/InstancePanel.svelte";
  import InstanceSettings from "./lib/InstanceSettings.svelte";
  import InstanceTile from "./lib/InstanceTile.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import LogView from "./lib/LogView.svelte";
  import Mods from "./lib/Mods.svelte";
  import Modal from "./lib/Modal.svelte";
  import Screenshots from "./lib/Screenshots.svelte";
  import Servers from "./lib/Servers.svelte";
  import Worlds from "./lib/Worlds.svelte";
  import Options from "./lib/Options.svelte";
  import NewInstance from "./lib/NewInstance.svelte";
  import Settings from "./lib/Settings.svelte";
  import TaskWindow from "./lib/TaskWindow.svelte";
  import { task, type Progress } from "./lib/task.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import { notify } from "./lib/toast.svelte";


  let instances = $state<Instance[]>([]);
  let accounts = $state<Account[]>([]);
  let selectedAccount = $state("");
  let selectedId = $state("");
  let search = $state("");
  /** How the grid is ordered; "recent" is what the backend already hands over. */
  let sort = $state<"recent" | "name" | "played">("recent");

  /** Launcher-wide preferences, loaded once; the settings dialog hands back what it saved. */
  let settings = $state<SettingsData | null>(null);

  let showNew = $state(false);
  let showSettings = $state(false);
  let showAccounts = $state(false);
  let editing = $state<Instance | null>(null);
  let managingMods = $state<Instance | null>(null);
  let managingWorlds = $state<Instance | null>(null);
  let editingOptions = $state<Instance | null>(null);
  let managingServers = $state<Instance | null>(null);
  let viewingShots = $state<Instance | null>(null);
  /** Which folder the content window is showing, so a drop lands in it. */
  let modsKind = $state<ModKind>("mods");
  /** Bumped when a dropped file lands, to make the content window re-read the folder. */
  let modsChanged = $state(0);

  /** Instance ids currently installing or running, with the label to show. */
  let busy = $state<Record<string, string>>({});
  let log = $state<string[]>([]);
  let showLog = $state(false);
  /**
   * Past logs and crash reports of the selected instance. Read when the panel
   * opens rather than kept fresh: the folder only changes when a game runs,
   * and re-reading it on every render would be a directory scan per frame.
   */
  let logFiles = $state<LogFile[]>([]);
  /** "" is the live output; otherwise "source/file" out of `logFiles`. */
  let logChoice = $state("");
  let logFileLines = $state<string[]>([]);
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

  /** Every instance that counts, so the number matches what the tiles say. */
  const totalPlayed = $derived(
    instances.reduce((sum, i) => sum + (i.count_play_time ? i.play_time : 0), 0)
  );

  /** Hours once there are any; a launcher total below a minute is not news. */
  function totalPlayTime(seconds: number) {
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes} min played`;
    return `${Math.floor(minutes / 60)} h played`;
  }
  const selected = $derived(visible.find((i) => i.id === selectedId));
  const activeAccount = $derived(accounts.find((a) => a.id === selectedAccount));

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

  // The list is per instance, so it is re-read whenever the panel opens or the
  // selection moves; the choice resets to live output because a file from the
  // previous instance is not in the new list.
  $effect(() => {
    const id = selected?.id;
    if (!showLog || !id) return;
    logChoice = "";
    logFileLines = [];
    api
      .listLogs(id)
      .then((files) => (logFiles = files))
      .catch((e) => notify(errorMessage(e), "error"));
  });

  async function openLog(choice: string) {
    logChoice = choice;
    if (!choice || !selected) return;
    const [source, ...rest] = choice.split("/");
    try {
      logFileLines = await api.readLog(selected.id, source as LogFile["source"], rest.join("/"));
    } catch (e) {
      notify(errorMessage(e), "error");
      logChoice = "";
    }
  }

  $effect(() => {
    refreshInstances();
    refreshAccounts();
    api.getSettings().then((s) => (settings = s));

    const unlisten = [
      // One subscription for the whole app: whoever started the download owns
      // the label, the window renders whatever is running.
      listen<Progress>("install-progress", (e) => task.report(e.payload)),
      listen<{ line: string }>("game-log", (e) => {
        // Cap the buffer: a long session emits tens of thousands of lines and
        // rendering all of them is what makes launcher log views crawl.
        log = [...log.slice(-2000), e.payload.line];
      }),
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
          showLog = true;
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

  async function importDropped(paths: string[]) {
    // While the content window is open, a dropped file belongs to the folder
    // it is showing — a jar for mods, a zip for resource packs and shaders.
    const extension = modsKind === "mods" ? ".jar" : ".zip";
    const content = paths.filter((p) => p.toLowerCase().endsWith(extension));
    if (managingMods && content.length) {
      for (const path of content) {
        try {
          notify(`Added ${await api.addModFile(managingMods.id, modsKind, path)}.`);
        } catch (e) {
          notify(errorMessage(e), "error");
        }
      }
      modsChanged++;
      return;
    }

    const packs = paths.filter((p) => /\.(zip|mrpack)$/i.test(p));
    if (packs.length === 0) {
      notify("Drop an exported instance .zip or a Modrinth .mrpack to import it.", "error");
      return;
    }
    for (const path of packs) {
      // A .mrpack downloads its whole mod list, so it needs the progress
      // window an exported .zip does not.
      const pack = path.toLowerCase().endsWith(".mrpack");
      if (pack) task.begin("Importing pack");
      try {
        const imported = await api.importInstance(path);
        notify(`Imported ${imported.name}.`);
        selectedId = imported.id;
      } catch (e) {
        notify(errorMessage(e), "error");
      } finally {
        if (pack) task.end();
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
    if (!selectedAccount) {
      showAccounts = true;
      notify("Add an account first.", "error");
      return;
    }
    busy[instance.id] = instance.installed ? "Starting…" : "Installing…";
    log = [];
    task.begin(instance.installed ? `Starting ${instance.name}` : `Installing ${instance.name}`);
    try {
      await api.launchInstance(instance.id, selectedAccount, quickPlay);
      busy[instance.id] = "Running";
      showLog = true;
      await refreshInstances();
    } catch (e) {
      delete busy[instance.id];
      restarting = "";
      notify(errorMessage(e), "error");
    } finally {
      task.end();
    }
  }

  /**
   * Open the content window on the folder that instance can actually use:
   * a vanilla instance has no loader, so it never starts on mods.
   */
  function openContent(instance: Instance) {
    modsKind = instance.loader === "vanilla" ? "resourcepacks" : "mods";
    managingMods = instance;
  }

  function openMenu(instance: Instance, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation(); // otherwise the grid's background menu replaces this one
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: busy[instance.id] ?? "Play",
          icon: "play",
          disabled: !!busy[instance.id],
          action: () => play(instance),
        },
        ...(busy[instance.id] === "Running"
          ? [
              {
                label: "Stop",
                icon: "stop" as const,
                action: () => actions.stopInstance(instance),
              },
              {
                label: "Restart",
                icon: "refresh" as const,
                action: () => restart(instance),
              },
            ]
          : []),
        {
          label: "Edit",
          icon: "sliders",
          disabled: !!busy[instance.id],
          action: () => (editing = instance),
        },
        { label: "Open Folder", icon: "folder", action: () => actions.openFolder(instance) },
        {
          label: "Content",
          icon: "package",
          action: () => openContent(instance),
        },
        {
          label: "Worlds",
          icon: "globe",
          action: () => (managingWorlds = instance),
        },
        {
          label: "Servers",
          icon: "server",
          action: () => (managingServers = instance),
        },
        {
          label: "Options",
          icon: "gear",
          action: () => (editingOptions = instance),
        },
        {
          label: "Screenshots",
          icon: "image",
          action: () => (viewingShots = instance),
        },
        ...(instance.loader !== "vanilla"
          ? [
              {
                label: "Mods Folder",
                icon: "folder" as const,
                action: () => actions.openModsFolder(instance, "mods"),
              },
            ]
          : []),
        {
          label: "Duplicate",
          icon: "copy",
          disabled: !!busy[instance.id],
          action: async () => {
            await actions.duplicateInstance(instance);
            await refreshInstances();
          },
        },
        { label: "Export", icon: "export", action: () => (exporting = instance) },
        {
          label: "Delete",
          icon: "trash",
          danger: true,
          disabled: !!busy[instance.id],
          action: () => (deleting = instance),
        },
      ],
    };
  }

  /** Right-click on empty space: the toolbar actions, where the cursor is. */
  function openBackgroundMenu(event: MouseEvent) {
    event.preventDefault();
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: "Add Instance", icon: "plus", action: () => (showNew = true) },
        {
          label: "Exports Folder",
          icon: "folder",
          action: () => api.openExportsFolder().catch((e) => notify(errorMessage(e), "error")),
        },
        {
          label: showLog ? "Hide Output" : "Output",
          icon: "import",
          action: () => (showLog = !showLog),
        },
        { label: "Settings", icon: "gear", action: () => (showSettings = true) },
      ],
    };
  }

  async function confirmDelete() {
    if (!deleting) return;
    await actions.deleteInstance(deleting);
    deleting = null;
    await refreshInstances();
  }
</script>

<div class="app">
  <!-- Top bar: identity, finding an instance, then the launcher-wide tools and
       who you are on the right; the grid below gets the full width. -->
  <header class="topbar">
    <div class="brand">
      <div class="mark" aria-hidden="true">
        <span></span><span></span><span></span><span></span>
      </div>
      <strong>JustLauncher</strong>
    </div>

    <div class="find">
      <Icon name="search" size={14} />
      <input bind:value={search} placeholder="Search instances" aria-label="Search instances" />
    </div>

    <span class="spacer"></span>

    <button class="primary new" onclick={() => (showNew = true)}>
      <Icon name="plus" size={14} />
      New instance
    </button>
    <button
      class="ghost icon"
      onclick={() => api.openExportsFolder().catch((e) => notify(errorMessage(e), "error"))}
      title="Exports folder"
      aria-label="Exports folder"
    >
      <Icon name="folder" />
    </button>
    <button
      class="ghost icon"
      class:on={showLog}
      onclick={() => (showLog = !showLog)}
      title="Game output"
      aria-label="Game output"
      aria-pressed={showLog}
    >
      <Icon name="import" />
    </button>
    <button
      class="ghost icon"
      onclick={() => (showSettings = true)}
      title="Settings"
      aria-label="Settings"
    >
      <Icon name="gear" />
    </button>
    <button class="ghost account" onclick={() => (showAccounts = true)} title="Accounts">
      {#if activeAccount}
        <img
          src="https://api.mineatar.io/face/{activeAccount.id}?scale=8"
          alt=""
          width="24"
          height="24"
        />
      {:else}
        <Icon name="user" />
      {/if}
      <span>{activeAccount ? activeAccount.name : "Add account"}</span>
    </button>
  </header>

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
              New instance
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
          onedit={(i) => (editing = i)}
          onmods={openContent}
          onworlds={(i) => (managingWorlds = i)}
          onservers={(i) => (managingServers = i)}
          onoptions={(i) => (editingOptions = i)}
          onscreenshots={(i) => (viewingShots = i)}
          onexport={(i) => (exporting = i)}
          onchanged={refreshInstances}
        />
      </aside>
    {/if}
  </div>

  {#if showLog}
    <LogView
      lines={logChoice ? logFileLines : log}
      logs={logFiles}
      choice={logChoice}
      onchoose={openLog}
      onclose={() => (showLog = false)}
    />
  {/if}

  <div class="status data">
    {#if selected}
      <span>Minecraft {selected.mc_version}</span>
      <span class="sep">·</span>
      <span>{loaderName(selected.loader)}</span>
      <span class="sep">·</span>
      <span>{selected.memory_mb} MB</span>
      {#if selected.java_path}
        <span class="sep">·</span>
        <span>custom Java</span>
      {/if}
    {/if}
    <span class="spacer"></span>
    {#if totalPlayed}
      <span>{totalPlayTime(totalPlayed)}</span>
      <span class="sep">·</span>
    {/if}
    <span>{instances.length} {instances.length === 1 ? "instance" : "instances"}</span>
  </div>

  <TaskWindow />

  {#if menu}
    <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
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

  {#if showSettings && settings}
    <Settings
      {settings}
      onclose={() => (showSettings = false)}
      onsaved={(saved) => (settings = saved)}
    />
  {/if}

  {#if viewingShots}
    <Screenshots instance={viewingShots} onclose={() => (viewingShots = null)} />
  {/if}

  {#if managingWorlds}
    <Worlds
      instance={managingWorlds}
      running={!!busy[managingWorlds.id]}
      onclose={() => (managingWorlds = null)}
      onplay={(folder) => play(managingWorlds!, { kind: "singleplayer", value: folder })}
    />
  {/if}

  {#if editingOptions}
    <Options
      instance={editingOptions}
      running={!!busy[editingOptions.id]}
      onclose={() => (editingOptions = null)}
    />
  {/if}

  {#if managingServers}
    <Servers
      instance={managingServers}
      running={!!busy[managingServers.id]}
      onclose={() => (managingServers = null)}
      onjoin={(address) => play(managingServers!, { kind: "multiplayer", value: address })}
    />
  {/if}

  {#if managingMods}
    <Mods
      instance={managingMods}
      changed={modsChanged}
      bind:kind={modsKind}
      onclose={() => (managingMods = null)}
    />
  {/if}

</div>

{#if showNew}
  <NewInstance onclose={() => (showNew = false)} oncreated={refreshInstances} />
{/if}

{#if showAccounts}
  <Accounts {accounts} onclose={() => (showAccounts = false)} onchange={refreshAccounts} />
{/if}

  {#if editing}
    <InstanceSettings
      instance={editing}
      onclose={() => (editing = null)}
      onsaved={refreshInstances}
      onexport={(i: Instance) => (exporting = i)}
    />
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

  .topbar {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 56px;
    flex: none;
    padding: 0 12px 0 16px;
    background: var(--bg-raised);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-right: 18px;
    font-size: 15px;
    white-space: nowrap;
  }

  /* The mark is a 2x2 block face, the same shape the instance icons use --
     the app signs itself with its own vocabulary rather than a logotype. */
  .mark {
    display: grid;
    grid-template: repeat(2, 7px) / repeat(2, 7px);
    gap: 2px;
    transform: rotate(45deg);
    margin: 0 4px;
  }

  .mark span {
    border-radius: 1.5px;
    background: var(--accent);
  }

  .mark span:nth-child(2) {
    background: var(--accent-lit);
  }

  .mark span:nth-child(4) {
    opacity: 0.55;
  }

  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    width: min(340px, 34vw);
    padding: 0 10px;
    background: var(--bg-inset);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-faint);
    transition: border-color 0.15s var(--ease);
  }

  .find:focus-within {
    border-color: var(--accent);
    color: var(--text-dim);
  }

  .find input {
    border: none;
    background: none;
    padding: 8px 0;
    font-size: 13px;
  }

  .find input:focus {
    outline: none;
  }

  .new {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-right: 6px;
    white-space: nowrap;
  }

  button.icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    padding: 0;
  }

  button.icon.on {
    color: var(--accent-lit);
    background: var(--accent-soft);
  }

  .account {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 180px;
    margin-left: 6px;
    padding: 5px 10px 5px 6px;
    color: var(--text);
    border: 1px solid var(--border);
  }

  .account span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account img {
    flex: none;
    border-radius: 4px;
    image-rendering: pixelated;
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
    width: 400px;
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

  /* One line of facts about what is selected -- the place the eye already
     goes on a desktop tool. */
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px;
    height: 30px;
    border-top: 1px solid var(--border);
    color: var(--text-faint);
    flex: none;
  }

  .sep {
    opacity: 0.5;
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
      width: 340px;
    }

    .new {
      display: none;
    }

    .account span {
      display: none;
    }
  }
</style>
