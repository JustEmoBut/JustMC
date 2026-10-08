<script lang="ts">
  import {
    api,
    errorMessage,
    loaderName,
    latestLabel,
    type FabricLoader,
    type Loader,
    type ManifestVersion,
  } from "./api";
  import Modal from "./Modal.svelte";
  import PackBrowser from "./PackBrowser.svelte";
  import { task } from "./task.svelte";
  import { notify } from "./toast.svelte";
  import { reportImport } from "./actions";

  let { onclose, oncreated }: { onclose: () => void; oncreated: () => Promise<void> } = $props();

  /** "form" builds an instance by hand; "browse" installs a Modrinth pack. */
  let mode = $state<"form" | "browse">("form");

  let name = $state("");
  let loader = $state<Loader>("vanilla");
  let showSnapshots = $state(false);
  let selected = $state("");
  let versions = $state<ManifestVersion[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  /** True once the user edits the name, so we stop auto-filling it. */
  let nameTouched = $state(false);
  /** Loader support for the selected version: null while in flight, "unknown"
      when the meta server could not be reached. */
  let loaderOk = $state<boolean | "unknown" | null>(null);
  /** Builds of the chosen loader for the chosen Minecraft version. */
  let loaders = $state<FabricLoader[]>([]);
  /** Empty means "whatever install time resolves", which is the usual answer. */
  let loaderVersion = $state("");

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

  // No loader covers every Minecraft version, and each project's own metadata
  // is the authority on which. An empty answer is a warning, never a block:
  // when Minecraft renumbered itself in 2026 the launcher briefly mapped
  // NeoForge's build numbers wrong, and a hard block turned that into "you
  // cannot create an instance at all".
  $effect(() => {
    const version = selected;
    const chosen = loader;
    if (chosen === "vanilla" || !version) {
      loaderOk = null;
      loaders = [];
      return;
    }
    loaderOk = null;
    // A build is only listed for the versions it supports, so a pin cannot
    // survive a change of either.
    loaders = [];
    loaderVersion = "";
    let current = true;
    api
      .listLoaders(version, chosen)
      .then((list) => {
        if (!current) return;
        loaders = list;
        loaderOk = list.length > 0;
      })
      // A blip on the meta server must not read as "unsupported"; let the user
      // through and leave the real error to install time.
      .catch(() => current && (loaderOk = "unknown"));
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

  /**
   * Import a Modrinth pack the user picked from disk.
   *
   * The file input gives content rather than a path — a webview never exposes
   * one — so the bytes go over IPC and the backend stages them.
   */
  async function importPack(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    // Clear it straight away so picking the same file twice still fires.
    input.value = "";
    if (!file) return;

    busy = true;
    task.begin(`Importing ${file.name}`);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const imported = await api.importArchiveBytes(file.name, bytes);
      reportImport(imported);
      await oncreated();
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      task.end();
      busy = false;
    }
  }

  async function create() {
    busy = true;
    try {
      await api.createInstance(name, selected, loader, loaderVersion || null);
      await oncreated();
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      busy = false;
    }
  }
</script>


<Modal
  title={mode === "browse" ? "Modrinth modpacks" : "New instance"}
  {onclose}
  width={mode === "browse" ? "880px" : "440px"}
>
  {#if mode === "form"}
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
        {#each [["vanilla", "Vanilla"], ["fabric", "Fabric"], ["quilt", "Quilt"], ["forge", "Forge"], ["neoforge", "NeoForge"]] as [value, text] (value)}
          <button
            class:active={loader === value}
            onclick={() => (loader = value as Loader)}
          >
            {text}
          </button>
        {/each}
      </div>
    </div>

    {#if loader !== "vanilla" && loaders.length}
      <div class="field">
        <label for="loader-version">{loaderName(loader)} loader</label>
        <select id="loader-version" bind:value={loaderVersion}>
          <!-- Fabric and Forge flag the one build they recommend; Quilt flags
               nothing and ships betas as its normal channel, so its default is
               the newest. -->
          <option value="">{latestLabel(loader)}</option>
          {#each loaders as l (l.version)}
            <option value={l.version}>{l.version}{l.stable ? " · recommended" : ""}</option>
          {/each}
        </select>
      </div>
    {/if}

    {#if loader !== "vanilla" && loaderOk === false}
      <p class="warn">
        {loaderName(loader)} lists no build for Minecraft {selected}. You can still create
        this instance, but it will not install until one exists.
      </p>
    {:else if loader !== "vanilla" && loaderOk === "unknown"}
      <p class="warn">Could not reach the {loaderName(loader)} meta server; support is unverified.</p>
    {/if}

    <div class="field">
      <label for="inst-name">Name</label>
      <input id="inst-name" bind:value={name} oninput={() => (nameTouched = true)} />
    </div>

    <div class="field import">
      <label for="pack-file">Or install a modpack</label>
      <div class="pack-row">
        <input
          id="pack-file"
          type="file"
          accept=".mrpack,.zip"
          disabled={busy}
          onchange={importPack}
        />
        <button onclick={() => (mode = "browse")}>Browse Modrinth…</button>
      </div>
      <p class="faint">
        A Modrinth <code>.mrpack</code>, or an instance <code>.zip</code> exported
        from JustLauncher. Everything above is ignored — the pack brings its own
        version, loader and mods.
      </p>
    </div>

  {:else}
    <PackBrowser onback={() => (mode = "form")} {oncreated} {onclose} />
  {/if}

  <!-- A snippet only reaches the component as a prop when it is a direct child
       of it. Nested inside the mode switch above, this one was silently never
       passed and the dialog had no buttons at all. -->
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    {#if mode === "form"}
      <button
        class="primary"
        onclick={create}
        disabled={busy || loading || !name.trim() ||
          (loader !== "vanilla" && loaderOk === null)}
      >
        {busy ? "Creating…" : "Create"}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .import {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .import input[type="file"] {
    font-size: 12.5px;
  }

  .import p {
    margin: 8px 0 0;
    line-height: 1.5;
  }

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

  .pack-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .pack-row input[type="file"] {
    flex: 1;
    font-size: 12.5px;
  }

</style>
