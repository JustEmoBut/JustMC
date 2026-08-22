<script lang="ts">
  import { untrack } from "svelte";
  import {
    api,
    errorMessage,
    type Instance,
    type ModFile,
    type ModKind,
    type ModHit,
    type ModProject,
    type ModUpdate,
    type ModVersion,
  } from "./api";
  import Icon from "./Icon.svelte";
  import { renderMarkdown } from "./markdown";
  import Modal from "./Modal.svelte";
  import { task } from "./task.svelte";
  import UpdateReview from "./UpdateReview.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    changed,
    kind = $bindable("mods"),
    onclose,
  }: {
    instance: Instance;
    /** Counter the parent bumps when it drops a file into this instance. */
    changed: number;
    /** Which folder is open. Bound, so a drop on the window lands in it. */
    kind?: ModKind;
    onclose: () => void;
  } = $props();

  /**
   * The three folders this window manages.
   *
   * Mods need a loader, so a vanilla instance is offered the other two only —
   * resource packs and shaders work on plain Minecraft (shaders need Iris,
   * which is itself a mod, but the folder is read either way).
   */
  const KINDS = [
    ["mods", "Mods"],
    ["resourcepacks", "Resource packs"],
    ["shaderpacks", "Shaders"],
  ] as const;
  const kinds = $derived(
    instance.loader === "vanilla" ? KINDS.filter(([k]) => k !== "mods") : KINDS
  );

  /** Modrinth's search indices, in the order the picker offers them. */
  const SORTS = [
    ["relevance", "Relevance"],
    ["downloads", "Downloads"],
    ["follows", "Followers"],
    ["newest", "Newest"],
    ["updated", "Updated"],
  ] as const;

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

  let tab = $state<"installed" | "browse">("installed");

  // Installed
  let mods = $state<ModFile[]>([]);
  let updates = $state<ModUpdate[]>([]);
  let checking = $state(false);
  let selection = $state<Set<string>>(new Set());
  /** Modrinth project ids already in the folder, resolved by hash. */
  let installedIds = $state<Set<string>>(new Set());

  // Browse
  let query = $state("");
  let sort = $state("relevance");
  let category = $state("");
  let hideInstalled = $state(true);
  let hits = $state<ModHit[]>([]);
  let total = $state(0);
  let searching = $state(false);
  let selected = $state<ModProject | null>(null);
  let versions = $state<ModVersion[]>([]);
  let installing = $state("");
  /** Projects ticked in the results, to install in one go. */
  let picked = $state<Set<string>>(new Set());
  /** Updates the user is reviewing before they are applied. */
  let reviewing = $state<ModUpdate[] | null>(null);

  // Matched by project id, never by name: Modrinth lists Jade as "Jade 🔍"
  // while the jar calls itself "Jade", so title comparison silently fails.
  const shownHits = $derived(
    hideInstalled ? hits.filter((h) => !installedIds.has(h.project_id)) : hits
  );
  const updateFor = $derived(new Map(updates.map((u) => [u.file, u])));

  $effect(() => {
    changed;
    kind;
    refresh();
  });

  // Switching folders is a different catalogue and a different list: anything
  // carried over would describe the folder the user just left.
  $effect(() => {
    kind;
    untrack(() => {
      selection = new Set();
      picked = new Set();
      updates = [];
      selected = null;
      hits = [];
      total = 0;
      if (!CATEGORIES[kind].includes(category)) category = "";
    });
  });

  // Re-run the search whenever a filter changes; the query itself waits for
  // Enter or the button, so typing does not fire a request per keystroke.
  $effect(() => {
    sort;
    category;
    kind;
    if (tab === "browse") search(true);
  });

  // The installed set arrives after the first search, and hiding it can empty
  // a list that looked full a moment ago. Top up instead of leaving two rows
  // and a button.
  $effect(() => {
    if (
      tab === "browse" &&
      !searching &&
      hits.length > 0 &&
      hits.length < total &&
      hits.length < CEILING &&
      visibleCount(hits) < ENOUGH
    ) {
      search(false);
    }
  });

  async function refresh() {
    try {
      mods = await api.listMods(instance.id, kind);
      selection = new Set([...selection].filter((f) => mods.some((m) => m.file === f)));
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    // Best effort: the folder still lists without the network, the browser
    // just cannot mark anything installed.
    try {
      installedIds = new Set(await api.installedModProjects(instance.id, kind));
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
          instance.loader
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

  async function open(hit: ModHit) {
    selected = null;
    versions = [];
    try {
      selected = await api.modProject(hit.project_id);
      versions = await api.modVersions(hit.project_id, instance.mc_version, kind, instance.loader);
    } catch (e) {
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
        files += (await api.installMod(instance.id, kind, project, null)).length;
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
    await refresh();
  }

  async function install(project: string, versionId: string | null = null) {
    installing = project;
    task.begin(`Installing ${selected?.title ?? noun(1)}`);
    try {
      const files = await api.installMod(instance.id, kind, project, versionId);
      notify(
        files.length > 1
          ? `Installed ${files.length} files, dependencies included.`
          : `Installed ${files[0]}.`
      );
      await refresh();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      task.end();
      installing = "";
    }
  }

  async function checkUpdates() {
    checking = true;
    try {
      updates = await api.checkModUpdates(instance.id, kind);
      notify(
        updates.length
          ? `${updates.length} ${noun(updates.length)} can be updated.`
          : "Everything is current."
      );
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      checking = false;
    }
  }

  /** Apply the reviewed updates one at a time, reporting each by name. */
  async function applyUpdates(chosen: ModUpdate[]) {
    reviewing = null;
    task.begin(`Updating ${chosen.length} ${noun(chosen.length)}`, chosen.length);
    const failed: string[] = [];

    for (const [i, update] of chosen.entries()) {
      task.step(`${update.name} → ${update.new_version}`, i);
      try {
        await api.updateMod(instance.id, kind, update.file, update.version_id);
        updates = updates.filter((u) => u.file !== update.file);
      } catch (e) {
        failed.push(update.name);
        notify(errorMessage(e), "error");
      }
    }

    task.end();
    const ok = chosen.length - failed.length;
    if (ok) notify(`Updated ${ok} ${noun(ok)}.`);
    await refresh();
  }

  /** Run an action over the selection, or over one mod when nothing is ticked. */
  async function bulk(action: (mod: ModFile) => Promise<unknown>, only?: ModFile) {
    const targets = only ? [only] : mods.filter((m) => selection.has(m.file));
    for (const mod of targets) {
      try {
        await action(mod);
      } catch (e) {
        notify(errorMessage(e), "error");
      }
    }
    selection = new Set();
    await refresh();
  }

  const toggle = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, !mod.enabled);
  const remove = (mod: ModFile) => api.deleteMod(instance.id, kind, mod.file);

  /** Deleting an unpacked pack wipes a whole tree, so those are confirmed
      first; a single file is one undo-able download away and is not. */
  let confirmingDelete = $state<{ targets: ModFile[]; only?: ModFile } | null>(null);
  function askRemove(only?: ModFile) {
    const targets = only ? [only] : mods.filter((m) => selection.has(m.file));
    if (targets.some((m) => m.dir)) confirmingDelete = { targets, only };
    else bulk(remove, only);
  }
  const enable = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, true);
  const disable = (mod: ModFile) => api.setModEnabled(instance.id, kind, mod.file, false);

  /** "mod" / "resource pack" / "shader", singular or plural, for messages. */
  function noun(n: number) {
    const one =
      kind === "mods" ? "mod" : kind === "resourcepacks" ? "resource pack" : "shader";
    return n === 1 ? one : `${one}s`;
  }

  function pick(file: string, on: boolean) {
    const next = new Set(selection);
    if (on) next.add(file);
    else next.delete(file);
    selection = next;
  }

  function size(n: number) {
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }

  /** "game-mechanics" -> "Game mechanics". The value sent to Modrinth is the
      slug; only the label changes. */
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

<Modal title="{kinds.find(([k]) => k === kind)?.[1] ?? 'Content'} — {instance.name}" {onclose} width="900px">
  {#if kinds.length > 1}
    <div class="folders segmented">
      {#each kinds as [value, text] (value)}
        <button class:active={kind === value} onclick={() => (kind = value)}>{text}</button>
      {/each}
    </div>
  {/if}

  <div class="tabs">
    <!-- One control with two positions, not two buttons: you are always in
         exactly one of these and the switch should look like it. -->
    <div class="segmented" role="tablist">
      <button
        role="tab"
        aria-selected={tab === "installed"}
        class:active={tab === "installed"}
        onclick={() => (tab = "installed")}
      >
        Installed <span class="badge">{mods.length}</span>
      </button>
      <button
        role="tab"
        aria-selected={tab === "browse"}
        class:active={tab === "browse"}
        onclick={() => {
          tab = "browse";
          if (!hits.length) search(true);
        }}
      >
        Browse
      </button>
    </div>
    <span class="spacer"></span>
    {#if tab === "installed"}
      {#if updates.length}
        <span class="pending">{updates.length} update{updates.length === 1 ? "" : "s"}</span>
        <button class="primary" onclick={() => (reviewing = updates)}>Update all</button>
      {/if}
      <button onclick={checkUpdates} disabled={checking || !mods.length}>
        {checking ? "Checking…" : "Check for updates"}
      </button>
    {/if}
  </div>

  {#if tab === "installed"}
    {#if selection.size}
      <div class="bulk">
        <span>{selection.size} selected</span>
        <button onclick={() => bulk(enable)}>Enable</button>
        <button onclick={() => bulk(disable)}>Disable</button>
        <button class="danger" onclick={() => askRemove()}>Delete</button>
        <button class="ghost" onclick={() => (selection = new Set())}>Clear</button>
      </div>
    {/if}

    <ul class="installed">
      {#each mods as mod (mod.file)}
        {@const update = updateFor.get(mod.file)}
        <li class:off={!mod.enabled} class:picked={selection.has(mod.file)}>
          <input
            type="checkbox"
            aria-label="Select {mod.name}"
            checked={selection.has(mod.file)}
            onchange={(e) => pick(mod.file, e.currentTarget.checked)}
          />
          {#if mod.icon}
            <img src={mod.icon} alt="" width="34" height="34" />
          {:else}
            <span class="slot" aria-hidden="true">{mod.name.slice(0, 1).toUpperCase()}</span>
          {/if}
          <span class="text">
            <strong>{mod.name}</strong>
            <span class="faint data">
              {mod.version || "unknown version"} · {mod.dir ? "folder" : size(mod.size)}
            </span>
          </span>
          <span class="file data faint">{mod.file}</span>
          {#if update}
            <button
              class="update"
              title="Show what changed"
              onclick={() => (reviewing = [update])}
            >
              <span class="dot"></span>
              {update.new_version}
            </button>
          {/if}
          <!-- A switch, because "is this mod on" is the question the row
               answers and a checkbox already means "is this row selected". -->
          <button
            class="switch"
            role="switch"
            aria-checked={mod.enabled}
            aria-label="{mod.enabled ? "Disable" : "Enable"} {mod.name}"
            onclick={() => bulk(toggle, mod)}
          >
            <span class="knob"></span>
          </button>
          <button class="ghost" aria-label="Delete {mod.name}" onclick={() => askRemove(mod)}>
            <Icon name="trash" />
          </button>
        </li>
      {:else}
        <li class="empty faint">
          No {noun(2)} yet. Browse Modrinth, or drop a
          {kind === "mods" ? ".jar" : ".zip"} on this window.
        </li>
      {/each}
    </ul>
  {:else}
    <div class="filters">
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
        {#each CATEGORIES[kind] as c (c)}
          <option value={c}>{label(c)}</option>
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
              onclick={() => open(hit)}
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
          <div class="sheet-head">
            {#if selected.icon_url}
              <img src={selected.icon_url} alt="" width="52" height="52" />
            {:else}
              <span class="slot big" aria-hidden="true">{selected.title.slice(0, 1)}</span>
            {/if}
            <div>
              <h3>{selected.title}</h3>
              <p class="cats">
                {#each selected.categories.slice(0, 4) as c (c)}
                  <span class="chip">{label(c)}</span>
                {/each}
              </p>
            </div>
          </div>

          <dl class="stats data">
            <div><dt>Downloads</dt><dd>{count(selected.downloads)}</dd></div>
            <div><dt>Followers</dt><dd>{count(selected.followers)}</dd></div>
            <div><dt>Builds</dt><dd>{versions.length}</dd></div>
          </dl>

          <p class="lede">{selected.description}</p>

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
            <button
              class="ghost"
              onclick={() => api.openUrl(`https://modrinth.com/mod/${selected!.slug}`)}
            >
              Modrinth page
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

          <!-- Author-written markdown, parsed and then sanitised; see
               markdown.ts for what is allowed through. -->
          <div class="body md">{@html renderMarkdown(selected.body)}</div>
        {:else}
          <p class="faint placeholder">Pick a mod to see what it does.</p>
        {/if}
      </div>
    </div>
  {/if}
</Modal>

{#if confirmingDelete}
  {@const targets = confirmingDelete.targets}
  <Modal title="Delete {noun(targets.length)}?" onclose={() => (confirmingDelete = null)}>
    <p>
      {targets.filter((m) => m.dir).length === 1 ? "One of these is" : "Some of these are"}
      an unpacked folder. Deleting removes the folder and everything inside it.
    </p>
    <ul class="targets">
      {#each targets as t (t.file)}
        <li>{t.file}{t.dir ? " (folder)" : ""}</li>
      {/each}
    </ul>
    {#snippet footer()}
      <button onclick={() => (confirmingDelete = null)}>Cancel</button>
      <button
        class="danger"
        onclick={() => {
          const { only } = confirmingDelete!;
          confirmingDelete = null;
          bulk(remove, only);
        }}
      >
        Delete
      </button>
    {/snippet}
  </Modal>
{/if}

{#if reviewing}
  <UpdateReview updates={reviewing} onclose={() => (reviewing = null)} onconfirm={applyUpdates} />
{/if}

<style>
  .targets {
    margin: 8px 0 0;
    padding-left: 18px;
  }

  .tabs,
  .filters,
  .bulk {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  /* A two-position switch cut from one piece: the well holds both, the active
     half is the raised one. */
  .folders {
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
    background: linear-gradient(var(--bg-panel), var(--bg-raised));
    border-color: var(--border-strong);
    box-shadow: var(--bevel);
    color: var(--text);
    font-weight: 600;
  }

  .badge {
    padding: 1px 6px;
    border-radius: 100px;
    background: rgb(0 0 0 / 0.4);
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--text-dim);
  }

  .pending {
    color: var(--xp);
    font-size: 12px;
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
    background: rgb(63 178 122 / 0.1);
    box-shadow: inset 0 0 0 1px rgb(63 178 122 / 0.25);
    border-radius: var(--radius);
    font-size: 12.5px;
  }

  ul {
    list-style: none;
  }

  .installed {
    max-height: 54vh;
    overflow-y: auto;
    overflow-x: hidden;
    padding-right: 4px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: var(--radius-sm);
    border-bottom: 1px solid rgb(255 255 255 / 0.04);
  }

  .installed li:hover {
    background: rgb(255 255 255 / 0.035);
  }

  .installed li.picked {
    background: rgb(63 178 122 / 0.1);
    box-shadow: inset 0 0 0 1px rgb(63 178 122 / 0.22);
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

  .slot.big {
    width: 52px;
    height: 52px;
    font-size: 21px;
  }

  .hits img,
  .installed img,
  .sheet-head img {
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

  /* A disabled mod still belongs in the list, just visibly out of the way. */
  .off .slot,
  .off .text {
    opacity: 0.5;
  }

  .off .text strong {
    text-decoration: line-through;
  }

  /* The jar name is the thing you would look for in the folder, so it fills
     the middle of the row rather than leaving it blank. */
  .file {
    flex: none;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10.5px;
    opacity: 0.6;
  }

  .switch {
    position: relative;
    width: 38px;
    height: 21px;
    flex: none;
    padding: 0;
    border-radius: 100px;
    background: var(--bg-inset);
    border-color: #1a1e24;
    box-shadow: var(--well);
  }

  .switch[aria-checked="true"] {
    background: linear-gradient(var(--accent), #2f8d5f);
    border-color: #2f8d5f;
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.2), 0 0 12px rgb(63 178 122 / 0.3);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 15px;
    height: 15px;
    border-radius: 100px;
    background: linear-gradient(#6b7480, #4b535d);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.5);
    transition: transform 0.12s ease-out, background 0.12s;
  }

  .switch[aria-checked="true"] .knob {
    transform: translateX(17px);
    background: linear-gradient(#ffffff, #d7dde3);
  }

  /* Gold is spent on one thing in this app: a build waiting to be installed. */
  .update {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 3px 10px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--xp);
    background: rgb(214 163 74 / 0.1);
    border-color: rgb(214 163 74 / 0.35);
    box-shadow: none;
  }

  .update:hover:not(:disabled) {
    background: rgb(214 163 74 / 0.2);
    border-color: var(--xp);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 100px;
    background: var(--xp);
    box-shadow: 0 0 8px var(--xp);
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
    background: rgb(63 178 122 / 0.07);
    box-shadow: inset 0 0 0 1px rgb(63 178 122 / 0.18);
  }

  .hits li > input[type="checkbox"] {
    margin-left: 6px;
  }

  .hits .current {
    background: rgb(63 178 122 / 0.14);
    box-shadow: inset 0 0 0 1px rgb(63 178 122 / 0.3);
  }

  .detail {
    overflow-y: auto;
    /* overflow-y alone computes overflow-x as auto, and a few pixels of
       overhang then draw a scrollbar across the pane. */
    overflow-x: hidden;
    padding: 16px;
    background: linear-gradient(var(--bg-panel), var(--bg-raised) 180px);
    border-radius: var(--radius);
    box-shadow: var(--bevel), inset 0 0 0 1px rgb(255 255 255 / 0.03);
  }

  .sheet-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .detail h3 {
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

  /* Three facts, set as a readout rather than a sentence, because they are the
     numbers people actually compare mods on. */
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

  /* Green means "this is already yours", the same thing the accent means on a
     selected instance. */
  .already {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 12px;
    border-radius: var(--radius-sm);
    background: rgb(63 178 122 / 0.12);
    box-shadow: inset 0 0 0 1px rgb(63 178 122 / 0.3);
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
    font-size: 12px;
    padding: 3px 9px;
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

  /* Project descriptions are full documents: headings, tables, screenshots,
     badge rows. They get a real type scale, one step below the app's own, so a
     mod's own hierarchy reads without competing with the window's. */
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

  .md :global(li) {
    display: list-item;
    padding: 0;
    border: 0;
    margin-bottom: 3px;
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
</style>
