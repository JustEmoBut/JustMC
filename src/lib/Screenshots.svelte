<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api, errorMessage, type Instance, type Screenshot } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    onclose,
    embedded = false,
  }: { instance: Instance; onclose: () => void; embedded?: boolean } = $props();

  let shots = $state<Screenshot[]>([]);
  let loading = $state(true);
  /** The one being shown full size, if any. */
  let viewing = $state<Screenshot | null>(null);

  async function refresh() {
    loading = true;
    try {
      shots = await api.listScreenshots(instance.id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    loading = false;
  }

  $effect(() => {
    refresh();
  });

  async function remove(shot: Screenshot) {
    try {
      await api.deleteScreenshot(instance.id, shot.file);
      if (viewing?.file === shot.file) viewing = null;
      shots = shots.filter((s) => s.file !== shot.file);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  function taken(at: number) {
    return at ? new Date(at * 1000).toLocaleString() : "";
  }
</script>

<Modal title="Screenshots — {instance.name}" {onclose} {embedded} width="860px">
  {#if loading}
    <p class="muted">Reading the screenshots folder…</p>
  {:else if shots.length === 0}
    <p class="muted">No screenshots yet. The game writes them with F2.</p>
  {:else if viewing}
    <button class="full" onclick={() => (viewing = null)} aria-label="Back to the grid">
      <img src={convertFileSrc(viewing.path)} alt={viewing.file} />
    </button>
    <p class="caption muted">{viewing.file} · {taken(viewing.taken)}</p>
  {:else}
    <div class="grid">
      {#each shots as shot (shot.file)}
        <figure>
          <button onclick={() => (viewing = shot)} aria-label="View {shot.file}">
            <!-- Served over the asset protocol: the bytes never cross IPC. -->
            <img src={convertFileSrc(shot.path)} alt={shot.file} loading="lazy" />
          </button>
          <figcaption>
            <span class="muted">{taken(shot.taken)}</span>
            <button
              class="destructive"
              onclick={() => remove(shot)}
              aria-label="Delete {shot.file}"
            >
              <Icon name="trash" size={13} />
            </button>
          </figcaption>
        </figure>
      {/each}
    </div>
  {/if}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 10px;
  }

  figure {
    background: var(--bg-inset);
    border-radius: var(--radius);
    overflow: hidden;
  }

  /* The thumbnail is the button, so the frame carries no padding of its own. */
  figure > button,
  .full {
    display: block;
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    box-shadow: none;
  }

  figure img {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: cover;
  }

  .full img {
    display: block;
    width: 100%;
    max-height: calc(100vh - 220px);
    object-fit: contain;
  }

  figcaption {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 10px;
    font-size: 11px;
  }

  figcaption .muted {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  figcaption button {
    display: flex;
    padding: 5px 7px;
  }

  .destructive:hover:not(:disabled) {
    color: var(--danger);
  }

  .caption {
    margin-top: 8px;
    text-align: center;
    font-size: 11px;
  }
</style>
