<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorMessage, type JavaInstall, type Settings } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    settings,
    onclose,
    embedded = false,
    onsaved,
  }: {
    settings: Settings;
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
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

<Modal title="Settings" {onclose} {embedded} width="480px">
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
      Close the launcher window
    </label>
    <p class="faint">
      Frees the memory the window uses. It reopens when the game exits; a game
      that hangs has to be ended before the launcher comes back.
    </p>
  </div>

  <div class="field section">
    <label for="g-cfkey">CurseForge API key</label>
    <input id="g-cfkey" type="password" bind:value={draft.curseforge_api_key} autocomplete="off" spellcheck="false" />
    <p class="faint">A Core API key from console.curseforge.com. Leave empty to keep CurseForge off.</p>
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
