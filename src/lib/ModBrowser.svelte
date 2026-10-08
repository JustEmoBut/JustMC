<script lang="ts">
  import { untrack } from "svelte";
  import {
    api,
    errorMessage,
    type Instance,
    type ModCategory,
    type ModHit,
    type ModKind,
    type ModProject,
    type ModSource,
    type ModVersion,
  } from "./api";
  import { SORTS, label, count, noun as nounOf } from "./format";
  import Icon from "./Icon.svelte";
  import ProjectSheet from "./ProjectSheet.svelte";
  import { task } from "./task.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    kind,
    listed,
    onchanged,
  }: {
    instance: Instance;
    kind: ModKind;
    /** Bumped by the parent each time it re-reads the folder. */
    listed: number;
    /** Re-read the folder after an install. */
    onchanged: () => Promise<void>;
  } = $props();

  const noun = (n: number) => nounOf(kind, n);

  /** Modrinth's categories per project type, from /v2/tag/category. */
  const CATEGORIES: Record<ModKind, string[]> = {
    mods: [
      "adventure", "cursed", "decoration", "economy", "equipment", "food",
      "game-mechanics", "library", "magic", "management", "minigame", "mobs",
      "optimization", "social", "storage", "technology", "transportation",
      "utility", "worldgen",
    ],
    resourcepacks: [
      "audio", "blocks", "combat", "core-shaders", "cursed", "decoration",
      "entities", "environment", "equipment", "fonts", "gui", "items",
      "locale", "modded", "models", "realistic", "simplistic", "themed",
      "tweaks", "utility", "vanilla-like",
      "8x-", "16x", "32x", "48x", "64x", "128x", "256x", "512x+",
    ],
    shaderpacks: [
      "atmosphere", "bloom", "cartoon", "colored-lighting", "cursed",
      "fantasy", "foliage", "high", "low", "medium", "path-tracing", "pbr",
      "potato", "realistic", "reflections", "screenshot", "semi-realistic",
      "shadows", "vanilla-like",
    ],
  };

  /** Which catalogue Browse searches. Project ids differ between the two, so
      everything keyed by one is reset when this changes. */
  let source = $state<ModSource>("modrinth");
  /** CurseForge needs the user's own key; without one its tab is disabled. */
  let curseforgeReady = $state(false);
  api
    .getSettings()
    .then((s) => (curseforgeReady = s.curseforge_api_key.trim() !== ""))
    .catch((e) => notify(errorMessage(e), "error"));

  /** Project ids of the browsed catalogue already in the folder, by hash. */
  let installedIds = $state<Set<string>>(new Set());

  let query = $state("");
  let sort = $state("relevance");
  /** A Modrinth slug, or a CurseForge category id; empty is every category. */
  let category = $state("");
  /** CurseForge's categories for the open folder, fetched when browsed. */
  let cfCategories = $state<ModCategory[]>([]);
  /** The picker's options as (value, label) for whichever catalogue is open. */
  const categoryOptions = $derived<[string, string][]>(
    source === "curseforge"
      ? cfCategories.map((c) => [c.id, c.name])
      : CATEGORIES[kind].map((c) => [c, label(c)])
  );
  let hideInstalled = $state(true);
  let hits = $state<ModHit[]>([]);
  let total = $state(0);
  let searching = $state(false);
  let selected = $state<ModProject | null>(null);
  let versions = $state<ModVersion[]>([]);
  let installing = $state("");
  /** Projects ticked in the results, to install in one go. */
  let picked = $state<Set<string>>(new Set());

  // Matched by project id, never by name: Modrinth lists Jade as "Jade 🔍"
  // while the jar calls itself "Jade", so title comparison silently fails.
  const shownHits = $derived(
    hideInstalled ? hits.filter((h) => !installedIds.has(h.project_id)) : hits
  );

  // Switching folders is a different catalogue and a different list: anything
  // carried over would describe the folder the user just left.
  $effect(() => {
    kind;
    source;
    untrack(() => {
      picked = new Set();
      selected = null;
      openId = "";
      hits = [];
      total = 0;
      // A Modrinth slug means nothing to CurseForge and the other way round.
      category = source === "modrinth" && CATEGORIES[kind].includes(category) ? category : "";
    });
  });

  // Re-run the search whenever a filter changes; the query itself waits for
  // Enter or the button, so typing does not fire a request per keystroke.
  $effect(() => {
    sort;
    category;
    kind;
    source;
    search(true);
  });

  // The installed set is in the browsed catalogue's ids, and changes with
  // every install or delete the parent reads back.
  $effect(() => {
    source;
    listed;
    untrack(() => markInstalled());
  });

  // Not cached: CurseForge's terms forbid keeping what its API returns.
  $effect(() => {
    if (source !== "curseforge") return;
    const forKind = kind;
    cfCategories = [];
    api
      .modCategories(forKind)
      .then((list) => {
        if (forKind === kind) cfCategories = list;
      })
      .catch((e) => notify(errorMessage(e), "error"));
  });

  // The installed set arrives after the first search, and hiding it can empty
  // a list that looked full a moment ago. Top up instead of leaving two rows
  // and a button.
  $effect(() => {
    if (
      !searching &&
      hits.length > 0 &&
      hits.length < total &&
      hits.length < CEILING &&
      visibleCount(hits) < ENOUGH
    ) {
      search(false);
    }
  });

  // Best effort: the folder still lists without the network, the browser
  // just cannot mark anything installed.
  async function markInstalled() {
    try {
      installedIds = new Set(await api.installedModProjects(instance.id, kind, source));
    } catch {
      installedIds = new Set();
    }
  }

  /** How many results are worth showing before the user has to ask for more. */
  const ENOUGH = 12;
  /** Stop topping up eventually, however heavily modded the instance is. */
  const CEILING = 200;

  /** Identifies the newest search, so an older one cannot append to it. */
  let run = 0;

  function visibleCount(list: ModHit[]) {
    return hideInstalled ? list.filter((h) => !installedIds.has(h.project_id)).length : list.length;
  }

  /**
   * Fetch results, and keep fetching while "hide installed" has eaten most of
   * them: on a heavily modded instance the first page can be almost entirely
   * filtered away, which left the list looking empty.
   *
   * The offset is the number of results already held rather than a page
   * counter, so appending can never drift out of step with the list, and a
   * stale run is dropped rather than merged into a newer one.
   */
  async function search(reset: boolean) {
    const id = ++run;
    searching = true;
    try {
      let collected = reset ? [] : hits;
      for (let page = 0; page < 5; page++) {
        const result = await api.searchMods(
          query,
          instance.mc_version,
          sort,
          category || null,
          collected.length,
          kind,
          instance.loader,
          source
        );
        if (id !== run) return; // a newer search replaced this one

        collected = [...collected, ...result.hits];
        hits = collected;
        total = result.total_hits;

        if (
          result.hits.length === 0 ||
          collected.length >= result.total_hits ||
          visibleCount(collected) >= ENOUGH
        ) {
          break;
        }
      }
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      if (id === run) searching = false;
    }
  }

  /** The project the sheet shows or is loading; a second click on it is a no-op. */
  let openId = "";

  /** Show a project; `reload` re-fetches the one already open. */
  async function open(project: string, reload = false) {
    if (project === openId && !reload) return;
    openId = project;
    selected = null;
    versions = [];
    try {
      const sheet = await api.modProject(project, source);
      const builds = await api.modVersions(project, instance.mc_version, kind, instance.loader, source);
      if (openId !== project) return; // another row was opened meanwhile
      selected = sheet;
      versions = builds;
    } catch (e) {
      // Forget it, so clicking the row again retries.
      if (openId === project) openId = "";
      notify(errorMessage(e), "error");
    }
  }

  function tick(project: string, on: boolean) {
    const next = new Set(picked);
    if (on) next.add(project);
    else next.delete(project);
    picked = next;
  }

  /**
   * Install everything ticked, one after another rather than at once: each mod
   * pulls its own dependencies and the download progress bar reports one batch
   * at a time. A failure is reported and the rest still go in.
   */
  async function installPicked() {
    const projects = [...picked];
    const failed: string[] = [];
    let files = 0;

    task.begin(`Installing ${projects.length} ${noun(projects.length)}`, projects.length);
    for (const [i, project] of projects.entries()) {
      installing = project;
      const title = hits.find((h) => h.project_id === project)?.title ?? project;
      task.step(title, i);
      try {
        files += (await api.installMod(instance.id, kind, project, null, source)).length;
      } catch (e) {
        failed.push(title);
        notify(errorMessage(e), "error");
      }
    }
    task.end();

    installing = "";
    picked = new Set();
    const ok = projects.length - failed.length;
    if (ok) notify(`Installed ${ok} ${noun(ok)} (${files} files).`);
    if (failed.length) notify(`Could not install: ${failed.join(", ")}.`, "error");
    await onchanged();
  }

  async function install(project: string, versionId: string | null = null) {
    installing = project;
    task.begin(`Installing ${selected?.title ?? noun(1)}`);
    try {
      const files = await api.installMod(instance.id, kind, project, versionId, source);
      notify(
        files.length > 1
          ? `Installed ${files.length} files, dependencies included.`
          : `Installed ${files[0]}.`
      );
      await onchanged();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      task.end();
      installing = "";
    }
  }
</script>

  <div class="filters">
    <div class="segmented" role="tablist" aria-label="Catalogue">
      <button
        role="tab"
        aria-selected={source === "modrinth"}
        class:active={source === "modrinth"}
        onclick={() => (source = "modrinth")}
      >
        Modrinth
      </button>
      <button
        role="tab"
        aria-selected={source === "curseforge"}
        class:active={source === "curseforge"}
        disabled={!curseforgeReady}
        title={curseforgeReady ? "" : "Add a CurseForge API key in Settings"}
        onclick={() => (source = "curseforge")}
      >
        CurseForge
      </button>
    </div>
    <input
      class="query"
      bind:value={query}
      placeholder="Search {noun(2)} for {instance.mc_version}"
      onkeydown={(e) => e.key === "Enter" && search(true)}
    />
    <select bind:value={sort} aria-label="Sort by">
      {#each SORTS as [value, text] (value)}
        <option {value}>{text}</option>
      {/each}
    </select>
    <select bind:value={category} aria-label="Category">
      <option value="">All categories</option>
      {#each categoryOptions as [value, text] (value)}
        <option {value}>{text}</option>
      {/each}
    </select>
    <label class="check">
      <input type="checkbox" bind:checked={hideInstalled} />
      Hide installed
    </label>
  </div>

  {#if picked.size}
    <div class="bulk">
      <span>{picked.size} queued</span>
      <span class="spacer"></span>
      <button class="ghost" onclick={() => (picked = new Set())}>Clear</button>
      <button class="primary" disabled={!!installing} onclick={installPicked}>
        {installing ? "Installing…" : `Install ${picked.size}`}
      </button>
    </div>
  {/if}

  <div class="browse">
    <ul class="hits">
      {#each shownHits as hit (hit.project_id)}
        <li class:current={selected?.id === hit.project_id} class:queued={picked.has(hit.project_id)}>
          {#if !installedIds.has(hit.project_id)}
            <input
              type="checkbox"
              aria-label="Queue {hit.title} for install"
              checked={picked.has(hit.project_id)}
              onchange={(e) => tick(hit.project_id, e.currentTarget.checked)}
            />
          {/if}
          <button
            class="ghost row"
            onclick={() => open(hit.project_id)}
            ondblclick={() =>
              !installedIds.has(hit.project_id) && tick(hit.project_id, !picked.has(hit.project_id))}
          >
            {#if hit.icon_url}
              <img src={hit.icon_url} alt="" width="34" height="34" />
            {:else}
              <span class="slot" aria-hidden="true">{hit.title.slice(0, 1)}</span>
            {/if}
            <span class="text">
              <strong>{hit.title}</strong>
              <span class="faint data">{count(hit.downloads)} ↓ · {hit.author}</span>
            </span>
            {#if installedIds.has(hit.project_id)}
              <span class="tick" title="Installed">✓</span>
            {/if}
          </button>
        </li>
      {:else}
        <li class="empty faint">{searching ? "Searching…" : "Nothing found."}</li>
      {/each}
      {#if hits.length < total}
        <li>
          <button class="more" onclick={() => search(false)} disabled={searching}>
            {searching ? "Loading…" : `Load more mods (${shownHits.length} of ${total})`}
          </button>
        </li>
      {/if}
    </ul>


    <div class="detail">
      {#if selected}
        <ProjectSheet project={selected} builds={versions.length}>
          <div class="row-buttons">
            {#if installedIds.has(selected.id)}
              <span class="already"><Icon name="play" size={11} /> Installed</span>
              <button disabled={!!installing} onclick={() => install(selected!.id)}>
                {installing === selected.id ? "Reinstalling…" : "Reinstall latest"}
              </button>
            {:else}
              <button class="primary" disabled={!!installing} onclick={() => install(selected!.id)}>
                {installing === selected.id ? "Installing…" : "Install latest"}
              </button>
            {/if}
            {#if versions.length}
              <select
                aria-label="Install a specific version"
                onchange={(e) => {
                  const id = e.currentTarget.value;
                  e.currentTarget.selectedIndex = 0;
                  if (id) install(selected!.id, id);
                }}
              >
                <option value="">Pick a version…</option>
                {#each versions as v (v.id)}
                  <option value={v.id}>
                    {v.version_number}{v.version_type === "release" ? "" : ` · ${v.version_type}`}
                  </option>
                {/each}
              </select>
            {/if}
          </div>

          <div class="links">
            <button class="ghost" onclick={() => open(selected!.id, true)} title="Load this page again">
              <Icon name="refresh" size={12} />
              Reload
            </button>
            <button
              class="ghost"
              onclick={() =>
                api.openUrl(selected!.page_url ?? `https://modrinth.com/mod/${selected!.slug}`)}
            >
              {source === "curseforge" ? "CurseForge page" : "Modrinth page"}
            </button>
            {#if selected.source_url}
              <button class="ghost" onclick={() => api.openUrl(selected!.source_url!)}>
                Source
              </button>
            {/if}
            {#if selected.issues_url}
              <button class="ghost" onclick={() => api.openUrl(selected!.issues_url!)}>
                Issues
              </button>
            {/if}
          </div>
        </ProjectSheet>
      {:else}
        <p class="faint placeholder">Pick a mod to see what it does.</p>
      {/if}
    </div>
  </div>

<style>
  .filters,
  .bulk {
    display: flex;
    align-items: center;
    gap: 8px;
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

  .spacer {
    flex: 1;
  }

  /* Only the search field stretches. Scoped to it by name: ".filters input"
     also caught the checkbox and blew it up to 140px wide. */
  .filters .query {
    flex: 1;
    min-width: 140px;
  }

  .filters select {
    width: auto;
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

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: var(--radius-sm);
    border-bottom: 1px solid rgb(255 255 255 / 0.04);
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

  .hits img {
    border-radius: var(--radius-sm);
    box-shadow: var(--well), 0 0 0 1px rgb(0 0 0 / 0.4);
    background: var(--bg-inset);
    flex: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 11px;
    flex: 1;
    min-width: 0;
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

  .browse {
    display: grid;
    grid-template-columns: 296px 1fr;
    gap: 16px;
    height: 54vh;
  }

  /* The results are a well you look into; the sheet is a panel that sits on
     top of it. Same relationship as the grid and the instance panel. */
  .hits {
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px;
    background: var(--bg-inset);
    box-shadow: var(--well);
    border-radius: var(--radius);
  }

  .hits li {
    border-bottom: 0;
  }

  .hits li:hover {
    background: rgb(255 255 255 / 0.04);
  }

  /* Queued rows are marked but stay readable: they are a shopping list, not a
     selection you act on by existing. */
  .hits .queued {
    background: rgb(34 197 94 / 0.07);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.18);
  }

  .hits li > input[type="checkbox"] {
    margin-left: 6px;
  }

  .hits .current {
    background: rgb(34 197 94 / 0.14);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.3);
  }

  .detail {
    overflow-y: auto;
    /* overflow-y alone computes overflow-x as auto, and a few pixels of
       overhang then draw a scrollbar across the pane. */
    overflow-x: hidden;
    padding: 16px;
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--bevel), inset 0 0 0 1px rgb(255 255 255 / 0.03);
  }

  /* Green means "this is already yours", the same thing the accent means on a
     selected instance. */
  .already {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 12px;
    border-radius: var(--radius-sm);
    background: rgb(34 197 94 / 0.12);
    box-shadow: inset 0 0 0 1px rgb(34 197 94 / 0.3);
    color: var(--accent-lit);
    font-size: 13px;
    font-weight: 600;
  }

  .tick {
    flex: none;
    color: var(--accent-lit);
    font-size: 13px;
  }

  .row-buttons,
  .links {
    display: flex;
    gap: 8px;
    margin: 14px 0;
    flex-wrap: wrap;
  }

  .row-buttons select {
    width: auto;
  }

  .links button {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    padding: 3px 9px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    font-size: 12.5px;
    font-weight: 400;
    font-stretch: 100%;
    letter-spacing: normal;
    text-transform: none;
    white-space: nowrap;
    color: var(--text-dim);
    cursor: pointer;
  }

  .empty,
  .placeholder {
    justify-content: center;
    padding: 30px;
    font-size: 12.5px;
    text-align: center;
    line-height: 1.5;
  }

  .more {
    width: 100%;
    margin: 8px 0;
  }

  /* In a window page the list takes whatever height is left. */
  :global(.embedded-page) .browse {
    flex: 1;
    min-height: 0;
    height: auto;
    max-height: none;
  }
</style>
