<script lang="ts">
  import { api, errorMessage, type Loader, type ManifestVersion } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let { onclose, oncreated }: { onclose: () => void; oncreated: () => Promise<void> } = $props();

  let name = $state("");
  let loader = $state<Loader>("vanilla");
  let showSnapshots = $state(false);
  let selected = $state("");
  let versions = $state<ManifestVersion[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  /** True once the user edits the name, so we stop auto-filling it. */
  let nameTouched = $state(false);
  /** Fabric support for the selected version: null while in flight, "unknown"
      when the meta server could not be reached. */
  let fabricOk = $state<boolean | "unknown" | null>(null);

  const shown = $derived(
    versions.filter((v) => showSnapshots || v.type === "release")
  );

  $effect(() => {
    // Keep the selection valid when the snapshot filter changes.
    if (shown.length && !shown.some((v) => v.id === selected)) {
      selected = shown[0].id;
    }
  });

  $effect(() => {
    if (!nameTouched) name = selected ? `Minecraft ${selected}` : "";
  });

  // Fabric does not cover every Minecraft version, and the meta server is the
  // only authority on which. Ask before letting the user create an instance
  // that could only fail at install time.
  $effect(() => {
    const version = selected;
    if (loader !== "fabric" || !version) {
      fabricOk = null;
      return;
    }
    fabricOk = null;
    let current = true;
    api
      .listFabricLoaders(version)
      .then((list) => current && (fabricOk = list.length > 0))
      // A blip on meta.fabricmc.net must not read as "unsupported"; let the
      // user through and leave the real error to install time.
      .catch(() => current && (fabricOk = "unknown"));
    return () => (current = false);
  });

  $effect(() => {
    api
      .listVersions()
      .then((list) => {
        versions = list.versions;
        selected = list.latest.release;
      })
      .catch((e) => notify(errorMessage(e), "error"))
      .finally(() => (loading = false));
  });

  async function create() {
    busy = true;
    try {
      await api.createInstance(name, selected, loader);
      await oncreated();
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="New instance" {onclose} width="440px">
  <div class="field">
    <label for="mc-version">Minecraft version</label>
    <select id="mc-version" bind:value={selected} disabled={loading}>
      {#each shown as v (v.id)}
        <option value={v.id}>{v.id}{v.type === "release" ? "" : ` · ${v.type}`}</option>
      {/each}
    </select>
    <label class="check">
      <input type="checkbox" bind:checked={showSnapshots} />
      Show snapshots and old versions
    </label>
  </div>

  <div class="field">
    <label for="loader">Mod loader</label>
    <div class="segmented" id="loader">
      {#each [["vanilla", "Vanilla"], ["fabric", "Fabric"]] as [value, text] (value)}
        <button
          class:active={loader === value}
          onclick={() => (loader = value as Loader)}
        >
          {text}
        </button>
      {/each}
    </div>
  </div>

  {#if loader === "fabric" && fabricOk === false}
    <p class="warn">Fabric has no loader for Minecraft {selected}. Pick another version.</p>
  {:else if loader === "fabric" && fabricOk === "unknown"}
    <p class="warn">Could not reach the Fabric meta server; support is unverified.</p>
  {/if}

  <div class="field">
    <label for="inst-name">Name</label>
    <input id="inst-name" bind:value={name} oninput={() => (nameTouched = true)} />
  </div>

  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button
      class="primary"
      onclick={create}
      disabled={busy || loading || !name.trim() || (loader === "fabric" && fabricOk === null) ||
        (loader === "fabric" && fabricOk === false)}
    >
      {busy ? "Creating…" : "Create"}
    </button>
  {/snippet}
</Modal>

<style>
  .warn {
    margin: -4px 0 14px;
    font-size: 12.5px;
    color: var(--danger);
  }

  /* A choice the user reads, not a field name — so it opts out of the
     stencilled uppercase treatment the global label rule applies. */
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0 0;
    font-size: 12.5px;
    font-weight: 400;
    font-stretch: 100%;
    letter-spacing: normal;
    text-transform: none;
    color: var(--text-dim);
    cursor: pointer;
  }



  .segmented {
    display: flex;
    gap: 4px;
    padding: 4px;
    background: var(--bg-inset);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .segmented button {
    flex: 1;
    background: transparent;
    border-color: transparent;
    padding: 6px;
    color: var(--text-dim);
  }

  .segmented button.active {
    background: var(--border);
    border-color: var(--border-strong);
    color: var(--text);
    font-weight: 600;
  }
</style>
