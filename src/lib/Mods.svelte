<script lang="ts">
  import { untrack } from "svelte";
  import {
    api,
    errorMessage,
    type Instance,
    type ModFile,
    type ModKind,
    type ModSource,
    type ModUpdate,
  } from "./api";
  import { noun as nounOf } from "./format";
  import Icon from "./Icon.svelte";
  import ModBrowser from "./ModBrowser.svelte";
  import Modal from "./Modal.svelte";
  import { task } from "./task.svelte";
  import UpdateReview from "./UpdateReview.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    changed,
    kind = $bindable("mods"),
    onclose,
    embedded = false,
  }: {
    instance: Instance;
    /** Counter the parent bumps when it drops a file into this instance. */
    changed: number;
    /** Which folder is open. Bound, so a drop on the window lands in it. */
    kind?: ModKind;
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
  } = $props();

  /**
   * The three folders this window manages.
   *
   * Mods need a loader, so a vanilla instance is offered the other two only —
   * resource packs and shaders work on plain Minecraft (shaders need Iris,
   * which is itself a mod, but the folder is read either way).
   */
  const KINDS = [
    ["mods", "Mods"],
    ["resourcepacks", "Resource packs"],
    ["shaderpacks", "Shaders"],
  ] as const;
  const kinds = $derived(
    instance.loader === "vanilla" ? KINDS.filter(([k]) => k !== "mods") : KINDS
  );

  let tab = $state<"installed" | "browse">("installed");

  // Installed
  let mods = $state<ModFile[]>([]);
  let updates = $state<ModUpdate[]>([]);
  let checking = $state(false);
  let selection = $state<Set<string>>(new Set());
  /** File name -> catalogues that recognise it. Absent means a local file. */
  let origins = $state<Record<string, ModSource[]>>({});
  /** Filters the folder listing; local only, nothing is re-read. */
  let filter = $state("");

  /** Bumped on every re-read, so the browser re-marks what is installed. */
  let listed = $state(0);
  /** Updates the user is reviewing before they are applied. */
  let reviewing = $state<ModUpdate[] | null>(null);
  const updateFor = $derived(new Map(updates.map((u) => [u.file, u])));

  // The file name is matched too: a hand-built jar often has no name of its own.
  const shownMods = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return mods;
    return mods.filter(
      (m) => m.name.toLowerCase().includes(q) || m.file.toLowerCase().includes(q)
    );
  });

  $effect(() => {
    changed;
    kind;
    refresh();
  });

  // Switching folders is a different list: a selection or an update carried
  // over would describe the folder the user just left.
  $effect(() => {
    kind;
    untrack(() => {
      selection = new Set();
      updates = [];
    });
  });

  async function refresh() {
    filter = "";
    try {
      mods = await api.listMods(instance.id, kind);
      selection = new Set([...selection].filter((f) => mods.some((m) => m.file === f)));
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    listed++;
    // Best effort, like the installed set: without the network every file
    // simply reads as local.
    try {
      origins = await api.modSources(instance.id, kind);
    } catch {
      origins = {};
    }
  }

  async function checkUpdates() {
    checking = true;
    try {
      updates = await api.checkModUpdates(instance.id, kind);
      notify(
        updates.length
          ? `${updates.length} ${noun(updates.length)} can be updated.`
          : "Everything is current."
      );
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      checking = false;
    }
  }

  /** Apply the reviewed updates one at a time, reporting each by name. */
  async function applyUpdates(chosen: ModUpdate[]) {
    reviewing = null;
    task.begin(`Updating ${chosen.length} ${noun(chosen.length)}`, chosen.length);
    const failed: string[] = [];

    for (const [i, update] of chosen.entries()) {
      task.step(`${update.name} → ${update.new_version}`, i);
      try {
        await api.updateMod(instance.id, kind, update.file, update.version_id);
        updates = updates.filter((u) => u.file !== update.file);
      } catch (e) {
        failed.push(update.name);
        notify(errorMessage(e), "error");
      }
    }

    task.end();
    const ok = chosen.length - failed.length;
    if (ok) notify(`Updated ${ok} ${noun(ok)}.`);
    await refresh();
  }

  /** Run an action over the selection, or over one mod when nothing is ticked. */
  async function bulk(action: (mod: ModFile) => Promise<unknown>, only?: ModFile) {
    const targets = only ? [only] : mods.filter((m) => selection.has(m.file));
    for (const mod of targets) {
      try {
        await action(mod);
      } catch (e) {
        notify(errorMessage(e), "error");
      }
    }
    selection = new Set();
    await refresh();
  }

  const toggle = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, !mod.enabled);
  const remove = (mod: ModFile) => api.deleteMod(instance.id, kind, mod.file);

  /** Deleting an unpacked pack wipes a whole tree, so those are confirmed
      first; a single file is one undo-able download away and is not. */
  let confirmingDelete = $state<{ targets: ModFile[]; only?: ModFile } | null>(null);
  function askRemove(only?: ModFile) {
    const targets = only ? [only] : mods.filter((m) => selection.has(m.file));
    if (targets.some((m) => m.dir)) confirmingDelete = { targets, only };
    else bulk(remove, only);
  }
  const enable = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, true);
  const disable = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, false);

  const noun = (n: number) => nounOf(kind, n);

  function pick(file: string, on: boolean) {
    const next = new Set(selection);
    if (on) next.add(file);
    else next.delete(file);
    selection = next;
  }

  function size(n: number) {
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }
</script>


<Modal title="{kinds.find(([k]) => k === kind)?.[1] ?? 'Content'} — {instance.name}" {onclose} {embedded} width="900px">
  <!-- Embedded, the window's own page list picks the folder. -->
  {#if kinds.length > 1 && !embedded}
    <div class="folders segmented">
      {#each kinds as [value, text] (value)}
        <button class:active={kind === value} onclick={() => (kind = value)}>{text}</button>
      {/each}
    </div>
  {/if}

  <div class="tabs">
    <!-- One control with two positions, not two buttons: you are always in
         exactly one of these and the switch should look like it. -->
    <div class="segmented" role="tablist">
      <button
        role="tab"
        aria-selected={tab === "installed"}
        class:active={tab === "installed"}
        onclick={() => (tab = "installed")}
      >
        Installed <span class="badge">{mods.length}</span>
      </button>
      <button
        role="tab"
        aria-selected={tab === "browse"}
        class:active={tab === "browse"}
        onclick={() => {
          tab = "browse";
        }}
      >
        Browse
      </button>
    </div>
    <span class="spacer"></span>
    {#if tab === "installed"}
      {#if updates.length}
        <span class="pending">{updates.length} update{updates.length === 1 ? "" : "s"}</span>
        <button class="primary" onclick={() => (reviewing = updates)}>Update all</button>
      {/if}
      <button onclick={checkUpdates} disabled={checking || !mods.length}>
        {checking ? "Checking…" : "Check for updates"}
      </button>
    {/if}
  </div>

  {#if tab === "installed"}
    {#if selection.size}
      <div class="bulk">
        <span>{selection.size} selected</span>
        <button onclick={() => bulk(enable)}>Enable</button>
        <button onclick={() => bulk(disable)}>Disable</button>
        <button class="danger" onclick={() => askRemove()}>Delete</button>
        <button class="ghost" onclick={() => (selection = new Set())}>Clear</button>
      </div>
    {/if}

    {#if mods.length}
      <div class="filters">
        <input
          class="query"
          type="search"
          bind:value={filter}
          aria-label="Filter installed {noun(2)}"
          placeholder="Filter installed {noun(2)}"
        />
        {#if filter.trim()}
          <span class="faint data">{shownMods.length} of {mods.length}</span>
        {/if}
      </div>
    {/if}

    <ul class="installed">
      {#each shownMods as mod (mod.file)}
        {@const update = updateFor.get(mod.file)}
        <li class:off={!mod.enabled} class:picked={selection.has(mod.file)}>
          <input
            type="checkbox"
            aria-label="Select {mod.name}"
            checked={selection.has(mod.file)}
            onchange={(e) => pick(mod.file, e.currentTarget.checked)}
          />
          {#if mod.icon}
            <img src={mod.icon} alt="" width="34" height="34" />
          {:else}
            <span class="slot" aria-hidden="true">{mod.name.slice(0, 1).toUpperCase()}</span>
          {/if}
          <span class="text">
            <strong>{mod.name}</strong>
            <span class="faint data">
              {mod.version || "unknown version"} · {mod.dir ? "folder" : size(mod.size)}
              {#each origins[mod.file] ?? [] as o (o)}
                <span class="origin">{o === "modrinth" ? "Modrinth" : "CurseForge"}</span>
              {:else}
                <span class="origin local" title="Neither Modrinth nor CurseForge recognises this file">Local</span>
              {/each}
            </span>
          </span>
          <span class="file data faint">{mod.file}</span>
          {#if update}
            <button
              class="update"
              title="Show what changed"
              onclick={() => (reviewing = [update])}
            >
              <span class="dot"></span>
              {update.new_version}
            </button>
          {/if}
          <!-- A switch, because "is this mod on" is the question the row
               answers and a checkbox already means "is this row selected". -->
          <button
            class="switch"
            role="switch"
            aria-checked={mod.enabled}
            aria-label="{mod.enabled ? "Disable" : "Enable"} {mod.name}"
            onclick={() => bulk(toggle, mod)}
          >
            <span class="knob"></span>
          </button>
          <button class="ghost" aria-label="Delete {mod.name}" onclick={() => askRemove(mod)}>
            <Icon name="trash" />
          </button>
        </li>
      {:else}
        <li class="empty faint">
          {#if mods.length}
            Nothing matches “{filter.trim()}”.
          {:else}
            No {noun(2)} yet. Browse Modrinth or CurseForge, or drop a
            {kind === "mods" ? ".jar" : ".zip"} on this window.
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <ModBrowser {instance} {kind} {listed} onchanged={refresh} />
  {/if}
</Modal>

{#if confirmingDelete}
  {@const targets = confirmingDelete.targets}
  <Modal title="Delete {noun(targets.length)}?" onclose={() => (confirmingDelete = null)}>
    <p>
      {targets.filter((m) => m.dir).length === 1 ? "One of these is" : "Some of these are"}
      an unpacked folder. Deleting removes the folder and everything inside it.
    </p>
    <ul class="targets">
      {#each targets as t (t.file)}
        <li>{t.file}{t.dir ? " (folder)" : ""}</li>
      {/each}
    </ul>
    {#snippet footer()}
      <button onclick={() => (confirmingDelete = null)}>Cancel</button>
      <button
        class="danger"
        onclick={() => {
          const { only } = confirmingDelete!;
          confirmingDelete = null;
          bulk(remove, only);
        }}
      >
        Delete
      </button>
    {/snippet}
  </Modal>
{/if}

{#if reviewing}
  <UpdateReview updates={reviewing} onclose={() => (reviewing = null)} onconfirm={applyUpdates} />
{/if}


<style>
  .targets {
    margin: 8px 0 0;
    padding-left: 18px;
  }

  .tabs,
  .filters,
  .bulk {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  /* A two-position switch cut from one piece: the well holds both, the active
     half is the raised one. */
  .folders {
    margin-bottom: 12px;
  }

  .segmented {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: var(--bg-inset);
    box-shadow: var(--well);
    border-radius: var(--radius);
  }

  .segmented button {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 5px 14px;
    background: none;
    border-color: transparent;
    box-shadow: none;
    color: var(--text-dim);
    font-size: 12.5px;
  }

  .segmented button:hover:not(.active) {
    background: rgb(255 255 255 / 0.05);
    border-color: transparent;
    color: var(--text);
  }

  .segmented .active {
    background: var(--bg-raised);
    border-color: var(--border-strong);
    box-shadow: var(--bevel);
    color: var(--text);
    font-weight: 600;
  }

  .badge {
    padding: 1px 6px;
    border-radius: 100px;
    background: rgb(0 0 0 / 0.4);
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--text-dim);
  }

  .pending {
    color: var(--xp);
    font-size: 12px;
    font-weight: 600;
  }

  .spacer {
    flex: 1;
  }

  /* Only the search field stretches. Scoped to it by name: ".filters input"
     also caught the checkbox and blew it up to 140px wide. */
  .filters .query {
    flex: 1;
    min-width: 140px;
  }


  .bulk {
    padding: 7px 10px;
    background: rgb(34 197 94 / 0.1);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.25);
    border-radius: var(--radius);
    font-size: 12.5px;
  }

  ul {
    list-style: none;
  }

  .installed {
    max-height: 54vh;
    overflow-y: auto;
    overflow-x: hidden;
    padding-right: 4px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: var(--radius-sm);
    border-bottom: 1px solid rgb(255 255 255 / 0.04);
  }

  .installed li:hover {
    background: rgb(255 255 255 / 0.035);
  }

  .installed li.picked {
    background: rgb(34 197 94 / 0.1);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.22);
  }

  li:last-child {
    border-bottom: 0;
  }

  /* Every mod gets a slot, filled with its initial when it has no icon, so the
     rows line up on a single left edge whether artwork loaded or not. */
  .slot {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: none;
    border-radius: var(--radius-sm);
    background: var(--bg-inset);
    box-shadow: var(--well);
    color: var(--text-faint);
    font-size: 14px;
    font-weight: 700;
  }


  .installed img {
    border-radius: var(--radius-sm);
    box-shadow: var(--well), 0 0 0 1px rgb(0 0 0 / 0.4);
    background: var(--bg-inset);
    flex: none;
  }


  .text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }

  .text strong {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text span {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A disabled mod still belongs in the list, just visibly out of the way. */
  .off .slot,
  .off .text {
    opacity: 0.5;
  }

  .off .text strong {
    text-decoration: line-through;
  }

  /* The jar name is the thing you would look for in the folder, so it fills
     the middle of the row rather than leaving it blank. */
  .file {
    flex: none;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10.5px;
    opacity: 0.6;
  }

  .switch {
    position: relative;
    width: 38px;
    height: 21px;
    flex: none;
    padding: 0;
    border-radius: 100px;
    background: var(--bg-inset);
    border-color: var(--border-strong);
    box-shadow: var(--well);
  }

  .switch[aria-checked="true"] {
    background: var(--accent);
    border-color: var(--accent);
    box-shadow: none;
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 15px;
    height: 15px;
    border-radius: 100px;
    background: #71717a;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.5);
    transition: transform 0.12s ease-out, background 0.12s;
  }

  .switch[aria-checked="true"] .knob {
    transform: translateX(17px);
    background: #fff;
  }

  /* Gold is spent on one thing in this app: a build waiting to be installed. */
  .update {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 3px 10px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--xp);
    background: rgb(214 163 74 / 0.1);
    border-color: rgb(214 163 74 / 0.35);
    box-shadow: none;
  }

  .update:hover:not(:disabled) {
    background: rgb(214 163 74 / 0.2);
    border-color: var(--xp);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 100px;
    background: var(--xp);
    box-shadow: 0 0 8px var(--xp);
  }













  /* Where an installed file is known from; a quiet tag after its size. */
  .origin {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 100px;
    background: rgb(255 255 255 / 0.055);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-dim);
  }

  .origin.local {
    background: none;
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.08);
  }













  .empty {
    justify-content: center;
    padding: 30px;
    font-size: 12.5px;
    text-align: center;
    line-height: 1.5;
  }

  /* In a window page the list takes whatever height is left. */
  :global(.embedded-page) .installed {
    flex: 1;
    min-height: 0;
    height: auto;
    max-height: none;
  }
</style>
