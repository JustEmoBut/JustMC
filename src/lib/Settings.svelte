<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorMessage, type JavaInstall, type Settings, type StorageReport } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    settings,
    onclose,
    onsaved,
  }: {
    settings: Settings;
    onclose: () => void;
    onsaved: (settings: Settings) => void;
  } = $props();

  // Edit a copy, like the instance dialog: cancelling has to leave the stored
  // settings untouched.
  let draft = $state<Settings>(untrack(() => ({ ...settings })));
  let javas = $state<JavaInstall[]>([]);
  let ramMb = $state<number | null>(null);

  const memoryMax = $derived(Math.min(16384, ramMb ?? 16384));

  $effect(() => {
    api.listJava().then((list) => (javas = list));
    api.systemMemoryMb().then((mb) => (ramMb = mb));
  });

  $effect(() => {
    if (draft.memory_mb > memoryMax) draft.memory_mb = memoryMax;
  });

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

  async function save() {
    try {
      const saved = { ...draft, memory_mb: Math.round(draft.memory_mb) };
      await api.saveSettings(saved);
      onsaved(saved);
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }
</script>

<Modal title="Settings" {onclose} width="480px">
  <p class="faint lead">
    These are the defaults a new instance starts with. Changing them never
    touches an instance that already exists — edit that one instead.
  </p>

  <div class="field">
    <label for="g-mem">Memory — {draft.memory_mb} MB</label>
    <input
      id="g-mem"
      type="range"
      min="1024"
      max={memoryMax}
      step="512"
      bind:value={draft.memory_mb}
    />
    <p class="faint">
      {#if ramMb}This machine has {Math.round(ramMb / 1024)} GB. {/if}More is not
      always better: Minecraft rarely benefits above 8 GB, and a large heap makes
      garbage collection pauses longer.
    </p>
  </div>

  <div class="field">
    <label for="g-java">Java</label>
    <select id="g-java" bind:value={draft.java_path}>
      <option value="">Detect automatically</option>
      {#each javas as java (java.path)}
        <option value={java.path}>Java {java.version} — {java.path}</option>
      {/each}
    </select>
    <p class="faint">
      Automatic matches each version's required Java, and downloads Mojang's own
      runtime when the machine has none. Pin one only if you need a specific JVM.
    </p>
  </div>

  <div class="field">
    <label for="g-args">Extra JVM arguments</label>
    <input id="g-args" bind:value={draft.jvm_args} placeholder="-XX:+UseG1GC" spellcheck="false" />
  </div>

  <div class="field section">
    <label for="g-minimise">While a game is running</label>
    <label class="check">
      <input id="g-minimise" type="checkbox" bind:checked={draft.minimise_on_play} />
      Minimise the launcher
    </label>
    <p class="faint">
      Restored when the game exits. The window is minimised rather than hidden,
      so it stays in the taskbar if the game never reports an exit.
    </p>
  </div>

  <div class="field section">
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

  {#snippet footer()}
    <span class="spacer"></span>
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save}>Save</button>
  {/snippet}
</Modal>

<style>
  input[type="range"] {
    padding: 0;
    background: transparent;
    border: none;
    accent-color: var(--accent);
  }

  .lead {
    margin: 0 0 18px;
    line-height: 1.5;
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

  /* Each of these is its own subject, not another default for a new
     instance, so a rule separates them from the fields above. */
  .section {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  /* A choice the user reads, not a field name — so it opts out of the
     stencilled uppercase treatment the global label rule applies. */
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 0;
    font-size: 12.5px;
    font-weight: 400;
    font-stretch: 100%;
    letter-spacing: normal;
    text-transform: none;
    color: var(--text-dim);
    cursor: pointer;
  }
</style>
