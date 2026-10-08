<script lang="ts">
  import { api, errorMessage, type Instance, type ModFile, type World } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    world,
    running,
    onclose,
  }: {
    instance: Instance;
    world: World;
    /** The game reads the folder while it runs; changing it under it is not safe. */
    running: boolean;
    onclose: () => void;
  } = $props();

  let packs = $state<ModFile[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  /** File whose delete is armed; only ever one. */
  let confirming = $state("");
  let picker = $state<HTMLInputElement>();

  async function refresh() {
    try {
      packs = await api.listDatapacks(instance.id, world.folder);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    loading = false;
  }

  $effect(() => {
    refresh();
  });

  /** Run one change, then read the folder again: it is the only record. */
  async function change(work: () => Promise<unknown>) {
    busy = true;
    confirming = "";
    try {
      await work();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    await refresh();
    busy = false;
  }

  /** The file input gives content, never a path, so the bytes cross IPC. */
  async function add(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = [...(input.files ?? [])];
    // Cleared straight away so picking the same file twice still fires.
    input.value = "";
    await change(async () => {
      for (const file of files) {
        await api.addDatapack(instance.id, world.folder, file.name, new Uint8Array(await file.arrayBuffer()));
      }
    });
  }
</script>

<Modal title="Data packs — {world.name}" {onclose} width="560px">
  {#if running}
    <p class="warn">{instance.name} is running. Stop it before changing data packs.</p>
  {/if}

  {#if loading}
    <p class="muted">Reading the datapacks folder…</p>
  {:else if packs.length === 0}
    <p class="muted">This world has no data packs.</p>
  {:else}
    <ul>
      {#each packs as pack (pack.file)}
        <li class:off={!pack.enabled}>
          {#if pack.icon}
            <img src={pack.icon} alt="" />
          {:else}
            <span class="placeholder"><Icon name="package" size={16} /></span>
          {/if}
          <div class="what">
            <strong>{pack.name}</strong>
            {#if pack.version}<span class="muted">{pack.version}</span>{/if}
          </div>
          {#if confirming === pack.file}
            <span class="muted">{pack.dir ? "Delete this folder?" : "Delete?"}</span>
            <button onclick={() => (confirming = "")}>Cancel</button>
            <button class="really" onclick={() => change(() => api.deleteDatapack(instance.id, world.folder, pack.file))}>
              Delete
            </button>
          {:else}
            <label class="check">
              <input
                type="checkbox"
                checked={pack.enabled}
                disabled={running || busy}
                onchange={() => change(() => api.setDatapackEnabled(instance.id, world.folder, pack.file, !pack.enabled))}
              />
              Enabled
            </label>
            <button
              class="destructive"
              disabled={running || busy}
              onclick={() => (confirming = pack.file)}
              aria-label="Delete {pack.name}"
            >
              <Icon name="trash" />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <p class="faint note">
    A pack added or removed here takes effect the next time the world is
    opened. Packs the world enabled in-game stay enabled in its save.
  </p>

  {#snippet footer()}
    <input bind:this={picker} type="file" accept=".zip" multiple onchange={add} hidden />
    <button class="add" disabled={running || busy} onclick={() => picker?.click()}>
      <Icon name="plus" />
      Add .zip…
    </button>
    <span class="spacer"></span>
    <button onclick={onclose}>Close</button>
  {/snippet}
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
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--bg-inset);
  }

  li.off {
    opacity: 0.6;
  }

  img,
  .placeholder {
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 4px;
    image-rendering: pixelated;
  }

  .placeholder {
    display: grid;
    place-items: center;
    color: var(--text-faint);
    background: var(--bg-raised);
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    color: var(--text-dim);
  }

  li button,
  .add {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  .note {
    margin-top: 12px;
    font-size: 12px;
    line-height: 1.5;
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
