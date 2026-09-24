<script lang="ts">
  import {
    api,
    errorMessage,
    loaderName,
    latestLabel,
    type FabricLoader,
    type Loader,
    type ManifestVersion,
    type ModHit,
    type ModProject,
    type ModVersion,
  } from "./api";
  import Modal from "./Modal.svelte";
  import { renderMarkdown } from "./markdown";
  import { task } from "./task.svelte";
  import { notify } from "./toast.svelte";

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

  // Browse: the pack catalogue. A pack brings its own Minecraft version and
  // loader, so unlike the in-instance browser there is nothing to narrow by.
  let packQuery = $state("");
  let packSort = $state("relevance");
  let packCategory = $state("");
  let packHits = $state<ModHit[]>([]);
  let packTotal = $state(0);
  let packSearching = $state(false);
  let pack = $state<ModProject | null>(null);
  let packBuilds = $state<ModVersion[]>([]);
  let installingPack = $state(false);

  /** Modrinth's search indices, in the order the picker offers them. */
  const SORTS = [
    ["relevance", "Relevance"],
    ["downloads", "Downloads"],
    ["follows", "Followers"],
    ["newest", "Newest"],
    ["updated", "Updated"],
  ] as const;

  /** Modrinth's game categories, from /v2/tag/category; packs share the set
      mods use. */
  const CATEGORIES = [
    "adventure", "cursed", "decoration", "economy", "equipment", "food",
    "game-mechanics", "magic", "management", "mobs", "optimization",
    "social", "storage", "technology", "transportation", "utility", "worldgen",
  ];

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
      notify(`Imported ${imported.name}.`);
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

  // ------------------------------------------------------------------ browse

  /** Identifies the newest search, so an older one cannot append to it. */
  let run = 0;

  async function searchPacks(reset: boolean) {
    const id = ++run;
    packSearching = true;
    try {
      const result = await api.searchModpacks(
        packQuery,
        packSort,
        packCategory || null,
        reset ? 0 : packHits.length
      );
      if (id !== run) return; // a newer search replaced this one
      packHits = reset ? result.hits : [...packHits, ...result.hits];
      packTotal = result.total_hits;
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      if (id === run) packSearching = false;
    }
  }

  // Entering the browse tab loads the first page once; after that the search
  // follows the filters, and the query itself waits for Enter like everywhere
  // else.
  let browsed = false;
  $effect(() => {
    packSort;
    packCategory;
    if (mode === "browse") {
      const first = !browsed;
      browsed = true;
      searchPacks(first);
    }
  });

  async function openPack(hit: ModHit) {
    pack = null;
    packBuilds = [];
    try {
      pack = await api.modProject(hit.project_id);
      packBuilds = await api.packVersions(hit.project_id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  /** Install a pack into a fresh instance and close the dialog on success. */
  async function installPack(versionId: string | null = null) {
    if (!pack) return;
    installingPack = true;
    task.begin(`Installing ${pack.title}`);
    try {
      const installed = await api.installModpack(pack.id, versionId);
      notify(`Installed ${installed.name}.`);
      await oncreated();
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      task.end();
      installingPack = false;
    }
  }

  function label(slug: string) {
    const words = slug.replace(/-/g, " ");
    return words.charAt(0).toUpperCase() + words.slice(1);
  }

  function count(n: number) {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1000) return `${Math.round(n / 1000)}k`;
    return `${n}`;
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
    <div class="pack-tools">
      <button class="ghost" onclick={() => (mode = "form")}>← Back</button>
      <input
        class="query"
        bind:value={packQuery}
        placeholder="Search modpacks"
        onkeydown={(e) => e.key === "Enter" && searchPacks(true)}
      />
      <select bind:value={packSort} aria-label="Sort by">
        {#each SORTS as [value, text] (value)}
          <option {value}>{text}</option>
        {/each}
      </select>
      <select bind:value={packCategory} aria-label="Category">
        <option value="">All categories</option>
        {#each CATEGORIES as c (c)}
          <option value={c}>{label(c)}</option>
        {/each}
      </select>
    </div>

    <div class="packs">
      <ul class="results">
        {#each packHits as hit (hit.project_id)}
          <li class:current={pack?.id === hit.project_id}>
            <button class="row" onclick={() => openPack(hit)}>
              {#if hit.icon_url}
                <img src={hit.icon_url} alt="" width="34" height="34" />
              {:else}
                <span class="slot" aria-hidden="true">{hit.title.slice(0, 1)}</span>
              {/if}
              <span class="text">
                <strong>{hit.title}</strong>
                <span class="faint data">{count(hit.downloads)} ↓ · {hit.author}</span>
              </span>
            </button>
          </li>
        {:else}
          <li class="faint empty">{packSearching ? "Searching…" : "Nothing found."}</li>
        {/each}
        {#if packHits.length < packTotal}
          <li>
            <button class="more" onclick={() => searchPacks(false)} disabled={packSearching}>
              {packSearching ? "Loading…" : `Load more (${packHits.length} of ${packTotal})`}
            </button>
          </li>
        {/if}
      </ul>

      <div class="sheet">
        {#if pack}
          <div class="pack-head">
            {#if pack.icon_url}
              <img src={pack.icon_url} alt="" width="52" height="52" />
            {:else}
              <span class="slot big" aria-hidden="true">{pack.title.slice(0, 1)}</span>
            {/if}
            <div>
              <h3>{pack.title}</h3>
              <p class="cats">
                {#each pack.categories.slice(0, 4) as c (c)}
                  <span class="chip">{label(c)}</span>
                {/each}
              </p>
            </div>
          </div>

          <dl class="stats data">
            <div><dt>Downloads</dt><dd>{count(pack.downloads)}</dd></div>
            <div><dt>Followers</dt><dd>{count(pack.followers)}</dd></div>
            <div><dt>Builds</dt><dd>{packBuilds.length}</dd></div>
          </dl>

          <p class="lede">{pack.description}</p>

          <div class="pack-buttons">
            <button
              class="primary"
              disabled={installingPack}
              onclick={() => installPack()}
            >
              {installingPack ? "Installing…" : "Install latest"}
            </button>
            {#if packBuilds.length}
              <select
                aria-label="Install a specific version"
                disabled={installingPack}
                onchange={(e) => {
                  const id = e.currentTarget.value;
                  e.currentTarget.selectedIndex = 0;
                  if (id) installPack(id);
                }}
              >
                <option value="">Pick a version…</option>
                {#each packBuilds as v (v.id)}
                  <option value={v.id}>
                    {v.version_number}{v.version_type === "release" ? "" : ` · ${v.version_type}`}
                  </option>
                {/each}
              </select>
            {/if}
            <button
              class="ghost"
              onclick={() => api.openUrl(`https://modrinth.com/modpack/${pack!.slug}`)}
            >
              Modrinth page
            </button>
          </div>

          <!-- Author-written markdown, parsed and then sanitised; see
               markdown.ts for what is allowed through. -->
          <div class="body md">{@html renderMarkdown(pack.body)}</div>
        {:else}
          <p class="faint placeholder">Pick a pack to see what it is.</p>
        {/if}
      </div>
    </div>
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

  /* ---- browse ---- */

  .pack-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .pack-row input[type="file"] {
    flex: 1;
    font-size: 12.5px;
  }

  .pack-tools {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  .pack-tools .query {
    flex: 1;
    min-width: 140px;
  }

  .pack-tools select {
    width: auto;
  }

  /* The same two-pane shape as the in-instance browser: a well of results on
     the left, the sheet that sits on it on the right. */
  .packs {
    display: grid;
    grid-template-columns: 300px 1fr;
    gap: 16px;
    height: 56vh;
  }

  .results {
    list-style: none;
    margin: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px;
    background: var(--bg-inset);
    box-shadow: var(--well);
    border-radius: var(--radius);
  }

  .results li {
    border-radius: var(--radius-sm);
  }

  .results .current {
    background: rgb(34 197 94 / 0.14);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.3);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    padding: 7px 6px;
    background: none;
    box-shadow: none;
    text-align: left;
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

  .slot.big {
    width: 52px;
    height: 52px;
    font-size: 21px;
  }

  .results img,
  .pack-head img {
    border-radius: var(--radius-sm);
    box-shadow: var(--well), 0 0 0 1px rgb(0 0 0 / 0.4);
    background: var(--bg-inset);
    flex: none;
  }

  .empty,
  .placeholder {
    display: flex;
    justify-content: center;
    padding: 30px 10px;
    font-size: 12.5px;
    text-align: center;
  }

  .more {
    width: 100%;
    margin: 8px 0;
  }

  .sheet {
    overflow-y: auto;
    /* overflow-y alone computes overflow-x as auto, and a few pixels of
       overhang then draw a scrollbar across the pane. */
    overflow-x: hidden;
    padding: 16px;
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--bevel), inset 0 0 0 1px rgb(255 255 255 / 0.03);
  }

  .pack-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .sheet h3 {
    margin: 0;
    font-size: 17px;
  }

  .cats {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin: 7px 0 0;
  }

  .chip {
    padding: 2px 8px;
    border-radius: 100px;
    background: rgb(255 255 255 / 0.055);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.05);
    font-size: 10.5px;
    font-weight: 600;
    font-stretch: 82%;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
  }

  .stats {
    display: flex;
    gap: 2px;
    margin: 16px 0;
    padding: 0;
    border-radius: var(--radius);
    overflow: hidden;
    background: rgb(0 0 0 / 0.25);
    box-shadow: var(--well);
  }

  .stats div {
    flex: 1;
    padding: 9px 12px;
  }

  .stats dt {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--text-faint);
  }

  .stats dd {
    margin: 3px 0 0;
    font-size: 15px;
    color: var(--text);
  }

  .lede {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-dim);
  }

  .pack-buttons {
    display: flex;
    gap: 8px;
    margin: 14px 0;
    flex-wrap: wrap;
  }

  .pack-buttons select {
    width: auto;
  }

  .body {
    margin-top: 4px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--text-dim);
    overflow-wrap: anywhere;
  }

  /* Pack descriptions are full documents; the same type scale the mod browser
     gives them, one step below the app's own. */
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 18px 0 8px;
    font-size: 14px;
    font-weight: 700;
    color: var(--text);
  }

  .md :global(h1) {
    font-size: 16px;
  }

  .md :global(h1:first-child),
  .md :global(h2:first-child),
  .md :global(h3:first-child) {
    margin-top: 0;
  }

  .md :global(p) {
    margin: 0 0 10px;
  }

  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 10px;
    padding-left: 20px;
  }

  .md :global(a) {
    color: var(--accent-lit);
    text-decoration: none;
  }

  .md :global(a:hover) {
    text-decoration: underline;
  }

  .md :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-sm);
    margin: 4px 0;
    vertical-align: middle;
  }

  .md :global(code) {
    padding: 1px 5px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.35);
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text);
  }

  .md :global(pre) {
    max-width: 100%;
    margin: 0 0 10px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-inset);
    box-shadow: var(--well);
    overflow-x: auto;
  }

  .md :global(pre code) {
    padding: 0;
    background: none;
  }

  .md :global(blockquote) {
    margin: 0 0 10px;
    padding-left: 12px;
    border-left: 2px solid var(--border-strong);
    color: var(--text-faint);
  }

  .md :global(hr) {
    margin: 16px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  .md :global(table) {
    width: 100%;
    max-width: 100%;
    display: block;
    overflow-x: auto;
    margin-bottom: 10px;
    border-collapse: collapse;
    font-size: 11.5px;
  }

  .md :global(th),
  .md :global(td) {
    padding: 5px 8px;
    border: 1px solid var(--border);
    text-align: left;
  }

  .md :global(th) {
    background: rgb(255 255 255 / 0.04);
    color: var(--text);
  }
</style>
