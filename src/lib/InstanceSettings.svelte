<script lang="ts">
  import { untrack } from "svelte";
  import * as actions from "./actions";
  import {
    api,
    errorMessage,
    type FabricLoader,
    type Instance,
    type JavaInstall,
  } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    onclose,
    onsaved,
  }: {
    instance: Instance;
    onclose: () => void;
    onsaved: () => Promise<void>;
  } = $props();

  // Edit a copy so cancelling leaves the list untouched. The snapshot is taken
  // once on open; later prop updates must not clobber what the user typed.
  let draft = $state<Instance>(untrack(() => ({ ...instance })));
  let javas = $state<JavaInstall[]>([]);
  /** Physical RAM in MB; the slider must not offer more than the machine has. */
  let ramMb = $state<number | null>(null);
  let loaders = $state<FabricLoader[]>([]);
  let confirmingDelete = $state(false);
  let exporting = $state(false);

  async function exportInstance() {
    exporting = true;
    try {
      await api.exportInstance(instance.id);
      notify(`Exported ${instance.name}. Opening the exports folder…`);
      await api.openExportsFolder();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      exporting = false;
    }
  }

  const memoryMax = $derived(Math.min(16384, ramMb ?? 16384));

  $effect(() => {
    api.listJava().then((list) => (javas = list));
    api.systemMemoryMb().then((mb) => (ramMb = mb));
    if (instance.loader === "fabric") {
      api
        .listFabricLoaders(instance.mc_version)
        .then((list) => (loaders = list))
        .catch(() => (loaders = []));
    }
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

  {#if instance.loader === "fabric"}
    <div class="field">
      <label for="s-loader">Fabric loader</label>
      <select id="s-loader" bind:value={draft.loader_version}>
        <option value="">Latest stable</option>
        <!-- Fabric's `stable` flag marks the one build it currently recommends,
             not a quality judgement on the rest, which are simply older. -->
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

  <div class="meta">
    <span>Minecraft {instance.mc_version}</span>
    <span>{instance.loader === "fabric" ? `Fabric ${instance.loader_version || ""}` : "Vanilla"}</span>
  </div>

  {#snippet footer()}
    {#if confirmingDelete}
      <span class="muted confirm">Delete this instance and all its worlds? Export it first if unsure.</span>
      <button onclick={() => (confirmingDelete = false)}>Cancel</button>
      <button class="danger" onclick={remove}>Delete</button>
    {:else}
      <button class="danger" onclick={() => (confirmingDelete = true)}>Delete</button>
      <button onclick={exportInstance} disabled={exporting}>
        {exporting ? "Exporting…" : "Export"}
      </button>
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" onclick={save}>Save</button>
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

  .field p {
    margin: 6px 0 0;
    line-height: 1.5;
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
