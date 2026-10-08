<script lang="ts">
  import { api, errorMessage, type StorageReport } from "./api";
  import { notify } from "./toast.svelte";

  /** Null until a scan has run: nothing is claimed about the store before. */
  let storage = $state<StorageReport | null>(null);
  let sweeping = $state(false);
  let swept = $state(false);

  async function scan() {
    sweeping = true;
    try {
      storage = await api.scanStorage();
      swept = false;
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    sweeping = false;
  }

  async function clean() {
    sweeping = true;
    try {
      const gone = await api.cleanStorage();
      notify(`Freed ${size(gone.total)}.`);
      storage = gone;
      swept = true;
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    sweeping = false;
  }

  function size(bytes: number) {
    const mb = bytes / 1024 / 1024;
    if (mb < 1) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
    return mb < 1024 ? `${Math.round(mb)} MB` : `${(mb / 1024).toFixed(1)} GB`;
  }

  /** What the sweep found, as the one line the button needs. */
  const found = $derived(
    !storage
      ? ""
      : [
          storage.versions.length && `${storage.versions.length} versions`,
          storage.asset_files && `${storage.asset_files.toLocaleString()} asset files`,
          storage.runtimes.length && `${storage.runtimes.length} Java runtimes`,
        ]
          .filter(Boolean)
          .join(", "),
  );

</script>

<div class="sweep">
  <div class="field">
    <label for="g-storage">Shared storage</label>
    <p class="faint">
      Versions, assets and Java runtimes are shared between instances and
      nothing removes them: a deleted instance leaves its download behind. Mod
      files and worlds are never touched by this.
    </p>
    <div class="storage">
      <button id="g-storage" onclick={scan} disabled={sweeping}>
        {sweeping ? "Working…" : "Scan"}
      </button>
      {#if storage}
        {#if storage.total === 0}
          <span class="muted">Nothing to remove.</span>
        {:else if swept}
          <span class="muted">Freed {size(storage.total)}.</span>
        {:else}
          <span class="muted">{size(storage.total)} — {found}</span>
          <button class="primary" onclick={clean} disabled={sweeping}>Delete</button>
        {/if}
      {/if}
    </div>
  </div>

</div>

<style>
  .sweep {
    padding: 20px;
  }

  .field p {
    margin: 6px 0 0;
    line-height: 1.5;
  }

  .storage {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }

  .storage .muted {
    font-size: 12px;
  }

</style>
