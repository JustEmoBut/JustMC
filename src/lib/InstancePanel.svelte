<script lang="ts">
  import * as actions from "./actions";
  import { loaderName, type Instance } from "./api";
  import BlockIcon from "./BlockIcon.svelte";
  import Icon from "./Icon.svelte";

  let {
    instance,
    status,
    onlaunch,
    onrestart,
    onedit,
    onmods,
    onworlds,
    onoptions,
    onservers,
    onscreenshots,
    onexport,
    onchanged,
  }: {
    instance: Instance;
    /** Non-empty while installing or running. */
    status: string | undefined;
    onlaunch: (instance: Instance) => void;
    onrestart: (instance: Instance) => void;
    onedit: (instance: Instance) => void;
    onmods: (instance: Instance) => void;
    onworlds: (instance: Instance) => void;
    onoptions: (instance: Instance) => void;
    onservers: (instance: Instance) => void;
    onscreenshots: (instance: Instance) => void;
    onexport: (instance: Instance) => void;
    onchanged: () => Promise<void>;
  } = $props();

  let duplicating = $state(false);
  let confirmingDelete = $state(false);

  // Reset the delete confirmation when the selection moves or the game starts,
  // so an armed confirmation can never fire against a different instance or a
  // running one.
  $effect(() => {
    instance.id;
    status;
    confirmingDelete = false;
  });

  async function duplicate() {
    duplicating = true;
    await actions.duplicateInstance(instance);
    duplicating = false;
    await onchanged();
  }

  async function remove() {
    await actions.deleteInstance(instance);
    await onchanged();
  }

  /**
   * Whole hours once there are any, minutes below that. A session shorter than
   * a minute still says something: silence there reads as a broken counter.
   */
  function playTime(seconds: number) {
    if (!seconds) return "";
    if (seconds < 60) return "under a minute played";
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes} min played`;
    return `${Math.floor(minutes / 60)} h ${minutes % 60} min played`;
  }

  function played(at: number) {
    if (!at) return "Never played";
    const days = Math.floor((Date.now() / 1000 - at) / 86400);
    if (days === 0) return "Played today";
    if (days === 1) return "Played yesterday";
    if (days < 30) return `Played ${days} days ago`;
    return `Played ${new Date(at * 1000).toLocaleDateString()}`;
  }
</script>

<section class="detail">
  <header class="hero">
    <BlockIcon name={instance.name} size={72} fabric={instance.loader !== "vanilla"} />
    <div class="title">
      <p class="eyebrow">{played(instance.last_played)}</p>
      <h1>{instance.name}</h1>
      <div class="chips">
        <span class="chip data">{instance.mc_version}</span>
        <span class="chip">{loaderName(instance.loader)}</span>
        {#if status}
          <span class="chip live">{status}</span>
        {/if}
      </div>
    </div>
  </header>

  <div class="actions">
    {#if status === "Running"}
      <button onclick={() => actions.stopInstance(instance)}>
        <Icon name="stop" size={14} />
        Stop
      </button>
      <button onclick={() => onrestart(instance)} title="Quit the game and start it again">
        <Icon name="refresh" size={14} />
        Restart
      </button>
    {:else}
      <button class="primary launch" disabled={!!status} onclick={() => onlaunch(instance)}>
        <Icon name="play" size={14} />
        {status ?? "Play"}
      </button>
    {/if}

    <span class="spacer"></span>

    <!-- What is done *to* the instance rather than opened inside it. -->
    <button
      class="ghost"
      disabled={!!status}
      title={status ? "Stop the game before editing this instance." : "Edit"}
      aria-label="Edit"
      onclick={() => onedit(instance)}
    >
      <Icon name="sliders" />
    </button>
    <button
      class="ghost"
      onclick={duplicate}
      disabled={duplicating || !!status}
      title={duplicating ? "Copying…" : "Duplicate"}
      aria-label="Duplicate"
    >
      <Icon name="copy" />
    </button>
    <button class="ghost" onclick={() => onexport(instance)} title="Export" aria-label="Export">
      <Icon name="export" />
    </button>
    <button
      class="ghost destructive"
      disabled={!!status}
      title={status ? "Stop the game before deleting this instance." : "Delete"}
      aria-label="Delete"
      onclick={() => (confirmingDelete = true)}
    >
      <Icon name="trash" />
    </button>
  </div>

  {#if confirmingDelete}
    <div class="confirm" role="alert">
      <p>Delete this instance and every world in it?</p>
      <button onclick={() => (confirmingDelete = false)}>Cancel</button>
      <button class="really" onclick={remove}>Delete</button>
    </div>
  {/if}

  <dl class="stats">
    <div>
      <dt>Minecraft</dt>
      <dd class="data">{instance.mc_version}</dd>
    </div>
    <div>
      <dt>Loader</dt>
      <dd>{loaderName(instance.loader)}</dd>
    </div>
    <div>
      <dt>Memory</dt>
      <dd class="data">{instance.memory_mb} MB</dd>
    </div>
    <div>
      <dt>Play time</dt>
      <dd>{playTime(instance.play_time) || "None yet"}</dd>
    </div>
  </dl>

  <!-- The panels an instance holds: cards rather than identical rows, so the
       eye picks one out by its icon instead of reading a list. -->
  <h2 class="eyebrow">Manage</h2>
  <div class="tiles">
    <button onclick={() => onmods(instance)}>
      <Icon name="package" size={18} />
      <strong>Content</strong>
      <span>Mods, resource packs, shaders</span>
    </button>
    <button onclick={() => onworlds(instance)}>
      <Icon name="globe" size={18} />
      <strong>Worlds</strong>
      <span>Back up, restore, play</span>
    </button>
    <button onclick={() => onservers(instance)}>
      <Icon name="server" size={18} />
      <strong>Servers</strong>
      <span>Multiplayer list</span>
    </button>
    <button onclick={() => onoptions(instance)}>
      <Icon name="gear" size={18} />
      <strong>Options</strong>
      <span>The game's options.txt</span>
    </button>
    <button onclick={() => onscreenshots(instance)}>
      <Icon name="image" size={18} />
      <strong>Screenshots</strong>
      <span>Everything you captured</span>
    </button>
    <button onclick={() => actions.openFolder(instance)}>
      <Icon name="folder" size={18} />
      <strong>Folder</strong>
      <span>Open in the file manager</span>
    </button>
  </div>
</section>

<style>
  .detail {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 32px 36px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
  }

  .title {
    min-width: 0;
  }

  h1 {
    margin: 4px 0 10px;
    font-size: 28px;
    line-height: 1.15;
    overflow-wrap: anywhere;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    padding: 3px 9px;
    border-radius: 100px;
    border: 1px solid var(--border-strong);
    font-size: 12px;
    color: var(--text-dim);
  }

  .chip.live {
    border-color: transparent;
    background: var(--accent-soft);
    color: var(--accent-lit);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .actions button {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
  }

  .actions button.ghost {
    width: 40px;
    justify-content: center;
    padding: 0;
  }

  /* The one filled control in the window: Play is the reason it exists. */
  .launch {
    min-width: 160px;
    justify-content: center;
    font-size: 15px;
  }

  .destructive:hover:not(:disabled) {
    background: rgb(248 113 113 / 0.1);
    color: var(--danger);
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid rgb(248 113 113 / 0.35);
    border-radius: var(--radius);
    background: rgb(248 113 113 / 0.07);
  }

  .confirm p {
    flex: 1;
    margin: 0;
    font-size: 13px;
    color: var(--text-dim);
  }

  .really {
    background: var(--danger);
    border-color: var(--danger);
    color: #2a0808;
  }

  .really:hover:not(:disabled) {
    background: #fca5a5;
    border-color: #fca5a5;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 1px;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--border);
    overflow: hidden;
  }

  .stats div {
    padding: 14px 16px;
    background: var(--bg-raised);
  }

  dt {
    font-size: 12px;
    color: var(--text-faint);
  }

  dd {
    margin: 4px 0 0;
    font-size: 15px;
    font-weight: 600;
  }

  dd.data {
    font-size: 14px;
  }

  h2.eyebrow {
    margin-bottom: -8px;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
    gap: 10px;
  }

  .tiles button {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 12px;
    row-gap: 2px;
    align-items: center;
    padding: 14px 16px;
    text-align: left;
    background: var(--bg-raised);
    border-color: var(--border);
    border-radius: var(--radius-lg);
    color: var(--text-dim);
  }

  .tiles button :global(svg) {
    grid-row: span 2;
    color: var(--text-faint);
    transition: color 0.15s var(--ease);
  }

  .tiles button:hover:not(:disabled) {
    background: var(--bg-panel);
    border-color: var(--border-strong);
  }

  .tiles button:hover :global(svg) {
    color: var(--accent-lit);
  }

  .tiles strong {
    font-size: 14px;
    color: var(--text);
  }

  .tiles span {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
