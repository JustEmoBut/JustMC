<script lang="ts">
  import { api, errorMessage, type Instance, type World } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    running,
    onclose,
  }: {
    instance: Instance;
    /** A world being written to while it is copied or deleted is not safe. */
    running: boolean;
    onclose: () => void;
  } = $props();

  let worlds = $state<World[]>([]);
  let loading = $state(true);
  let busy = $state("");
  /** Folder of the world whose delete button is armed; only ever one. */
  let confirming = $state("");

  async function refresh() {
    loading = true;
    try {
      worlds = await api.listWorlds(instance.id);
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

  function size(bytes: number) {
    const mb = bytes / 1024 / 1024;
    return mb < 1024 ? `${Math.max(1, Math.round(mb))} MB` : `${(mb / 1024).toFixed(1)} GB`;
  }

  function played(at: number) {
    if (!at) return "";
    return new Date(at * 1000).toLocaleString();
  }
</script>

<Modal title="Worlds — {instance.name}" {onclose} width="620px">
  {#if loading}
    <p class="muted">Reading the saves folder…</p>
  {:else if worlds.length === 0}
    <p class="muted">This instance has no worlds yet.</p>
  {:else}
    {#if running}
      <p class="warn">{instance.name} is running. Stop it before touching a world.</p>
    {/if}
    <ul>
      {#each worlds as world (world.folder)}
        <li>
          <div class="what">
            <strong>{world.name}</strong>
            <span class="muted">
              {size(world.size)}{#if played(world.last_played)} · {played(world.last_played)}{/if}
              {#if world.name !== world.folder} · {world.folder}{/if}
            </span>
          </div>

          {#if confirming === world.folder}
            <span class="muted">Delete permanently?</span>
            <button onclick={() => (confirming = "")}>Cancel</button>
            <button class="really" onclick={() => remove(world)}>Delete</button>
          {:else}
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
</Modal>

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
