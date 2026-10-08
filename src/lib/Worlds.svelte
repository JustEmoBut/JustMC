<script lang="ts">
  import { api, errorMessage, type Instance, type World, type WorldBackup } from "./api";
  import Datapacks from "./Datapacks.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    running,
    onclose,
    embedded = false,
    onplay,
  }: {
    instance: Instance;
    /** A world being written to while it is copied or deleted is not safe. */
    running: boolean;
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
    /** Launch straight into this save folder; the dialog closes behind it. */
    onplay: (folder: string) => void;
  } = $props();

  /**
   * Whether this version understands Quick Play, from its own metadata. Null
   * until the answer lands, so nothing is offered and nothing is denied yet.
   */
  let quickPlay = $state<boolean | null>(null);
  const canPlay = $derived(quickPlay === true && !running);

  $effect(() => {
    // An unreachable manifest or an unknown version leaves the button showing:
    // the launch path checks again and gives the honest error there.
    api
      .quickPlaySupported(instance.id)
      .then((yes) => (quickPlay = yes))
      .catch(() => (quickPlay = true));
  });

  function open(world: World) {
    if (!canPlay) return;
    // Launch first: closing clears the caller's reference to this instance, so
    // handing the folder over afterwards hands it to nothing.
    onplay(world.folder);
    onclose();
  }

  let worlds = $state<World[]>([]);
  let backups = $state<WorldBackup[]>([]);
  let loading = $state(true);
  let busy = $state("");
  /** Folder of the world whose delete button is armed; only ever one. */
  let confirming = $state("");
  /** Archive whose restore is armed. Restoring replaces a world, so it asks. */
  let confirmingRestore = $state("");
  /** The world whose data packs dialog is open. */
  let packsOf = $state<World | null>(null);

  async function refresh() {
    loading = true;
    try {
      worlds = await api.listWorlds(instance.id);
      backups = await api.listWorldBackups(instance.id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    loading = false;
  }

  $effect(() => {
    refresh();
  });

  async function backup(world: World) {
    busy = world.folder;
    try {
      await api.backupWorld(instance.id, world.folder);
      notify(`Backed up ${world.name}.`);
      // The list below is read from the exports folder, so a new backup only
      // appears if it is read again.
      await refresh();
      await api.openExportsFolder();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    busy = "";
  }

  async function remove(world: World) {
    busy = world.folder;
    confirming = "";
    try {
      await api.deleteWorld(instance.id, world.folder);
      notify(`Deleted ${world.name}.`);
      await refresh();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    busy = "";
  }

  async function restore(backup: WorldBackup) {
    busy = backup.file;
    confirmingRestore = "";
    try {
      const folder = await api.restoreWorld(instance.id, backup.file);
      notify(`Restored ${folder}.`);
      await refresh();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    busy = "";
  }

  function size(bytes: number) {
    const mb = bytes / 1024 / 1024;
    return mb < 1024 ? `${Math.max(1, Math.round(mb))} MB` : `${(mb / 1024).toFixed(1)} GB`;
  }

  /** Size, last played and the folder when it differs from the world's name. */
  function detail(world: World) {
    const parts = [size(world.size)];
    if (world.last_played) parts.push(new Date(world.last_played * 1000).toLocaleString());
    if (world.name !== world.folder) parts.push(world.folder);
    return parts.join(" · ");
  }
</script>

<Modal title="Worlds — {instance.name}" {onclose} {embedded} width="620px">
  {#if loading}
    <p class="muted">Reading the saves folder…</p>
  {:else if worlds.length === 0}
    <p class="muted">This instance has no worlds yet.</p>
  {:else}
    {#if running}
      <p class="warn">{instance.name} is running. Stop it before touching a world.</p>
    {:else if quickPlay === false}
      <p class="muted note">
        Opening a world from here needs Minecraft 1.20 or newer; this instance is
        on {instance.mc_version}.
      </p>
    {/if}
    <ul>
      {#each worlds as world (world.folder)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <li ondblclick={() => open(world)}>
          <div class="what">
            <strong>{world.name}</strong>
            <span class="muted">{detail(world)}</span>
          </div>

          {#if confirming === world.folder}
            <span class="muted">Delete permanently?</span>
            <button onclick={() => (confirming = "")}>Cancel</button>
            <button class="really" onclick={() => remove(world)}>Delete</button>
          {:else}
            {#if canPlay}
              <button onclick={() => open(world)} title="Launch straight into this world">
                <Icon name="play" />
                Play
              </button>
            {/if}
            <button onclick={() => (packsOf = world)} title="Add, disable or remove this world's data packs">
              <Icon name="package" />
              Data packs
            </button>
            <button
              disabled={running || !!busy}
              onclick={() => backup(world)}
              title="Zips the world into the exports folder"
            >
              <Icon name="export" />
              {busy === world.folder ? "Working…" : "Back up"}
            </button>
            <button
              class="destructive"
              disabled={running || !!busy}
              onclick={() => (confirming = world.folder)}
              aria-label="Delete {world.name}"
            >
              <Icon name="trash" />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if backups.length > 0}
    <h4>Backups</h4>
    <ul>
      {#each backups as backup (backup.file)}
        <li>
          <div class="what">
            <strong>{backup.folder}</strong>
            <span class="muted">
              {size(backup.size)} · {new Date(backup.made * 1000).toLocaleString()}
            </span>
          </div>
          {#if confirmingRestore === backup.file}
            <span class="muted">Replace {backup.folder} with this?</span>
            <button onclick={() => (confirmingRestore = "")}>Cancel</button>
            <button class="really" onclick={() => restore(backup)}>Restore</button>
          {:else}
            <button
              disabled={running || !!busy}
              onclick={() => (confirmingRestore = backup.file)}
              title="Unpacks the backup over the world it was taken from"
            >
              <Icon name="import" />
              {busy === backup.file ? "Working…" : "Restore"}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Modal>

{#if packsOf}
  <Datapacks {instance} world={packsOf} {running} onclose={() => (packsOf = null)} />
{/if}

<style>
  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--bg-inset);
  }

  /* The base button style is not flex, so an icon and a label would stack. */
  li button {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .what strong {
    font-size: 13px;
  }

  .what .muted {
    font-size: 11px;
  }

  h4 {
    margin: 16px 0 8px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .note {
    margin-bottom: 10px;
    font-size: 12px;
  }

  .warn {
    margin-bottom: 10px;
    font-size: 12px;
    color: #fbbf24;
  }

  .really,
  .destructive:hover:not(:disabled) {
    color: var(--danger);
  }
</style>
