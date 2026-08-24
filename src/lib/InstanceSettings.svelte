<script lang="ts">
  import { untrack } from "svelte";
  import * as actions from "./actions";
  import {
    api,
    errorMessage,
    type FabricLoader,
    loaderName,
    latestLabel,
    type Instance,
    type JavaInstall,
    type ManifestVersion,
  } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    onclose,
    onsaved,
    onexport,
  }: {
    instance: Instance;
    onclose: () => void;
    onsaved: () => Promise<void>;
    onexport: (instance: Instance) => void;
  } = $props();

  // Edit a copy so cancelling leaves the list untouched. The snapshot is taken
  // once on open; later prop updates must not clobber what the user typed.
  let draft = $state<Instance>(untrack(() => ({ ...instance })));
  let javas = $state<JavaInstall[]>([]);
  /** Physical RAM in MB; the slider must not offer more than the machine has. */
  let ramMb = $state<number | null>(null);
  let loaders = $state<FabricLoader[]>([]);
  let versions = $state<ManifestVersion[]>([]);
  // Start the filter open when the instance itself is on a snapshot, so its
  // own version is visible in the list without a click.
  let showSnapshots = $state(untrack(() => !/^\d+\.\d+(\.\d+)?$/.test(instance.mc_version)));
  let confirmingDelete = $state(false);

  const memoryMax = $derived(Math.min(16384, ramMb ?? 16384));
  const changedVersion = $derived(draft.mc_version !== instance.mc_version);
  // Keep the instance's own version listed even when the snapshot filter would
  // hide it, so opening the dialog never silently reselects something else.
  const shownVersions = $derived(
    versions.filter((v) => showSnapshots || v.type === "release" || v.id === instance.mc_version)
  );
  const loaderUnsupported = $derived(
    draft.loader !== "vanilla" && changedVersion && loaders.length === 0
  );

  $effect(() => {
    api.listJava().then((list) => (javas = list));
    api.systemMemoryMb().then((mb) => (ramMb = mb));
    api.listVersions().then((list) => (versions = list.versions)).catch(() => (versions = []));
  });

  // The loader list is per Minecraft version, so it has to follow the draft,
  // not the saved instance: picking a version the loader never supported must
  // show up here rather than at install time.
  $effect(() => {
    if (draft.loader === "vanilla") return;
    const version = draft.mc_version;
    const loader = draft.loader;
    let current = true;
    api
      .listLoaders(version, loader)
      .then((list) => current && (loaders = list))
      .catch(() => current && (loaders = []));
    return () => (current = false);
  });

  // A loader build is only listed for the versions it supports, so a pinned one
  // cannot survive a version change.
  $effect(() => {
    draft.mc_version;
    untrack(() => {
      if (draft.mc_version !== instance.mc_version) draft.loader_version = "";
    });
  });

  // A machine can shrink (or the setting can arrive from an exported instance),
  // so clamp rather than trusting what was stored.
  $effect(() => {
    if (draft.memory_mb > memoryMax) draft.memory_mb = memoryMax;
  });

  async function save() {
    try {
      await api.updateInstance({ ...draft, memory_mb: Math.round(draft.memory_mb) });
      await onsaved();
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  /** Same wording as the side panel, so one instance reads the same in both. */
  function playTime(seconds: number) {
    if (!seconds) return "never played";
    if (seconds < 60) return "played under a minute";
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `played ${minutes} min`;
    return `played ${Math.floor(minutes / 60)} h ${minutes % 60} min`;
  }

  async function remove() {
    await actions.deleteInstance(instance);
    await onsaved();
    onclose();
  }
</script>

<Modal title={instance.name} {onclose} width="480px">
  <div class="field">
    <label for="s-name">Name</label>
    <input id="s-name" bind:value={draft.name} />
  </div>

  <div class="field">
    <label for="s-version">Minecraft version</label>
    <select id="s-version" bind:value={draft.mc_version} disabled={!versions.length}>
      {#each shownVersions as v (v.id)}
        <option value={v.id}>{v.id}{v.type === "release" ? "" : ` · ${v.type}`}</option>
      {/each}
    </select>
    <label class="check">
      <input type="checkbox" bind:checked={showSnapshots} />
      Show snapshots
    </label>
    {#if loaderUnsupported}
      <p class="warn">
        {loaderName(draft.loader)} has no loader for {draft.mc_version}. Pick another version.
      </p>
    {:else if changedVersion}
      <p class="warn">
        The new version downloads on the next launch. Worlds and configs stay,
        but mods built for {instance.mc_version} will most likely stop working.
      </p>
    {/if}
  </div>

  <div class="field">
    <label for="s-mem">Memory — {draft.memory_mb} MB</label>
    <input
      id="s-mem"
      type="range"
      min="1024"
      max={memoryMax}
      step="512"
      bind:value={draft.memory_mb}
    />
    <p class="faint">
      {#if ramMb}This machine has {Math.round(ramMb / 1024)} GB. {/if}More is not always better: Minecraft rarely benefits above 8 GB, and a large
      heap makes garbage collection pauses longer.
    </p>
  </div>

  {#if draft.loader !== "vanilla"}
    <div class="field">
      <label for="s-loader">{loaderName(draft.loader)} loader</label>
      <select id="s-loader" bind:value={draft.loader_version}>
        <option value="">{latestLabel(draft.loader)}</option>
        <!-- Fabric's `stable` flag marks the one build it currently recommends,
             not a quality judgement on the rest, which are simply older. Quilt
             publishes no such flag, so nothing there is labelled. -->
        {#each loaders as l (l.version)}
          <option value={l.version}>{l.version}{l.stable ? " · recommended" : ""}</option>
        {/each}
      </select>
      <p class="faint">
        Changing this downloads the new loader profile on the next launch; the
        instance's files stay where they are.
      </p>
    </div>
  {/if}

  <div class="field">
    <label for="s-java">Java</label>
    <select id="s-java" bind:value={draft.java_path}>
      <option value="">Detect automatically</option>
      {#each javas as java (java.path)}
        <option value={java.path}>Java {java.version} — {java.path}</option>
      {/each}
    </select>
  </div>

  <div class="field">
    <label for="s-args">Extra JVM arguments</label>
    <input id="s-args" bind:value={draft.jvm_args} placeholder="-XX:+UseG1GC" spellcheck="false" />
  </div>

  <div class="field">
    <label for="s-width">Window size</label>
    <div class="pair">
      <input
        id="s-width"
        type="number"
        min="0"
        placeholder="Width"
        bind:value={draft.window_width}
      />
      <span class="muted">×</span>
      <input
        id="s-height"
        type="number"
        min="0"
        placeholder="Height"
        bind:value={draft.window_height}
      />
    </div>
    <p class="faint">
      Left empty the game opens at whatever size it last remembered. Set both to
      open at a fixed size — useful for recording, or on a screen the default
      window is lost on.
    </p>
  </div>

  <div class="field">
    <label for="s-pre">Before launch</label>
    <input id="s-pre" bind:value={draft.pre_launch} placeholder="backup.bat" spellcheck="false" />
    <p class="faint">
      Run in the game folder before the game starts; the launch is cancelled if
      it fails. <code>$INST_DIR</code>, <code>$INST_MC_DIR</code>,
      <code>$INST_ID</code>, <code>$INST_NAME</code> and
      <code>$INST_MC_VERSION</code> are set in the environment (use
      <code>%NAME%</code> on Windows). Output goes to the launcher log.
    </p>
  </div>

  <div class="field">
    <label for="s-post">After the game exits</label>
    <input id="s-post" bind:value={draft.post_exit} placeholder="sync-saves.sh" spellcheck="false" />
  </div>

  <div class="field">
    <label for="s-count">Play time</label>
    <label class="check">
      <input id="s-count" type="checkbox" bind:checked={draft.count_play_time} />
      Count sessions here towards play time
    </label>
    <p class="faint">
      Turn it off for an instance kept for testing. What is already recorded
      stays; only new sessions stop being added.
    </p>
  </div>

  <div class="meta">
    <span>Minecraft {instance.mc_version}</span>
    <span>{playTime(instance.play_time)}{instance.count_play_time ? "" : " · not counted"}</span>
    <span>
      {instance.loader === "vanilla"
        ? "Vanilla"
        : `${loaderName(instance.loader)} ${instance.loader_version || ""}`}
    </span>
  </div>

  {#snippet footer()}
    {#if confirmingDelete}
      <span class="muted confirm">Delete this instance and all its worlds? Export it first if unsure.</span>
      <button onclick={() => (confirmingDelete = false)}>Cancel</button>
      <button class="danger" onclick={remove}>Delete</button>
    {:else}
      <button class="danger" onclick={() => (confirmingDelete = true)}>Delete</button>
      <button onclick={() => onexport(instance)}>Export</button>
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" onclick={save} disabled={loaderUnsupported}>Save</button>
    {/if}
  {/snippet}
</Modal>

<style>
  input[type="range"] {
    padding: 0;
    background: transparent;
    border: none;
    accent-color: var(--accent);
  }

  /* Width × height on one line: two boxes and the sign between them. */
  .pair {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .field p {
    margin: 6px 0 0;
    line-height: 1.5;
  }

  .warn {
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

  .meta {
    display: flex;
    gap: 8px;
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    color: var(--text-faint);
  }

  .meta span + span::before {
    content: "·";
    margin-right: 8px;
  }

  .confirm {
    flex: 1;
    font-size: 13px;
    align-self: center;
  }
</style>
