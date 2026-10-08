<script lang="ts">
  import { api, errorMessage, type ModHit, type ModProject, type ModVersion } from "./api";
  import { SORTS, count, label } from "./format";
  import Icon from "./Icon.svelte";
  import ProjectSheet from "./ProjectSheet.svelte";
  import { task } from "./task.svelte";
  import { notify } from "./toast.svelte";

  let {
    onback,
    oncreated,
    onclose,
  }: {
    onback: () => void;
    oncreated: () => Promise<void>;
    onclose: () => void;
  } = $props();

  // The pack catalogue. A pack brings its own Minecraft version and loader,
  // so unlike the in-instance browser there is nothing to narrow by.
  let packQuery = $state("");
  let packSort = $state("relevance");
  let packCategory = $state("");
  let packHits = $state<ModHit[]>([]);
  let packTotal = $state(0);
  let packSearching = $state(false);
  let pack = $state<ModProject | null>(null);
  let packBuilds = $state<ModVersion[]>([]);
  let installingPack = $state(false);
  /** Modrinth's game categories, from /v2/tag/category; packs share the set
      mods use. */
  const CATEGORIES = [
    "adventure", "cursed", "decoration", "economy", "equipment", "food",
    "game-mechanics", "magic", "management", "mobs", "optimization",
    "social", "storage", "technology", "transportation", "utility", "worldgen",
  ];
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

  // The first page loads on opening; after that the search follows the
  // filters, and the query itself waits for Enter like everywhere else.
  let browsed = false;
  $effect(() => {
    packSort;
    packCategory;
    const first = !browsed;
    browsed = true;
    searchPacks(first);
  });

  /** The pack the sheet shows or is loading; a second click on it is a no-op. */
  let openId = "";

  /** Show a pack; `reload` re-fetches the one already open. */
  async function openPack(project: string, reload = false) {
    if (project === openId && !reload) return;
    openId = project;
    pack = null;
    packBuilds = [];
    try {
      const sheet = await api.modProject(project, "modrinth");
      const builds = await api.packVersions(project);
      if (openId !== project) return; // another row was opened meanwhile
      pack = sheet;
      packBuilds = builds;
    } catch (e) {
      // Forget it, so clicking the row again retries.
      if (openId === project) openId = "";
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
</script>

  <div class="pack-tools">
    <button class="ghost" onclick={onback}>← Back</button>
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
          <button class="row" onclick={() => openPack(hit.project_id)}>
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
        <ProjectSheet project={pack} builds={packBuilds.length}>
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
            <button class="ghost reload" onclick={() => openPack(pack!.id, true)} title="Load this page again">
              <Icon name="refresh" size={12} />
              Reload
            </button>
          </div>
        </ProjectSheet>
      {:else}
        <p class="faint placeholder">Pick a pack to see what it is.</p>
      {/if}
    </div>
  </div>

<style>
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

  /* The sheet's own styles are ProjectSheet's; this is only its frame. */
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

  .pack-buttons {
    display: flex;
    gap: 8px;
    margin: 14px 0;
    flex-wrap: wrap;
  }

  .pack-buttons select {
    width: auto;
  }

  .reload {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
