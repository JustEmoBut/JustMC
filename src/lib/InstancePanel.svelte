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
    onopen,
    onexport,
    onchanged,
  }: {
    instance: Instance;
    /** Non-empty while installing or running. */
    status: string | undefined;
    onlaunch: (instance: Instance) => void;
    onrestart: (instance: Instance) => void;
    /** Open the instance window on one of its pages. */
    onopen: (instance: Instance, page: string) => void;
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
    <BlockIcon name={instance.name} size={56} fabric={instance.loader !== "vanilla"} />
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

  <!-- Prism's instance toolbar: one column of actions, the launch on top. -->
  <div class="actions">
    {#if status === "Running"}
      <button onclick={() => actions.stopInstance(instance)}>
        <Icon name="stop" size={14} />
        Kill
      </button>
      <button onclick={() => onrestart(instance)} title="Quit the game and start it again">
        <Icon name="refresh" size={14} />
        Restart
      </button>
    {:else}
      <button class="primary launch" disabled={!!status} onclick={() => onlaunch(instance)}>
        <Icon name="play" size={14} />
        {status ?? "Launch"}
      </button>
    {/if}

    <hr />
    <button onclick={() => onopen(instance, "settings")}>
      <Icon name="sliders" size={14} />
      Edit
    </button>
    {#if instance.loader !== "vanilla"}
      <button onclick={() => onopen(instance, "mods")}>
        <Icon name="puzzle" size={14} />
        Mods
      </button>
    {/if}
    <button onclick={() => onopen(instance, "worlds")}>
      <Icon name="globe" size={14} />
      Worlds
    </button>
    <button onclick={() => onopen(instance, "servers")}>
      <Icon name="server" size={14} />
      Servers
    </button>
    <button onclick={() => onopen(instance, "screenshots")}>
      <Icon name="image" size={14} />
      Screenshots
    </button>
    <button onclick={() => onopen(instance, "log")}>
      <Icon name="terminal" size={14} />
      Log
    </button>
    <button onclick={() => actions.openFolder(instance)}>
      <Icon name="folder" size={14} />
      Folder
    </button>

    <hr />
    <!-- What is done *to* the instance rather than opened inside it. -->
    <button onclick={() => onexport(instance)}>
      <Icon name="export" size={14} />
      Export
    </button>
    <button onclick={duplicate} disabled={duplicating || !!status}>
      <Icon name="copy" size={14} />
      {duplicating ? "Copying…" : "Copy"}
    </button>
    <button
      class="destructive"
      disabled={!!status}
      title={status ? "Stop the game before deleting this instance." : undefined}
      onclick={() => (confirmingDelete = true)}
    >
      <Icon name="trash" size={14} />
      Delete
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

</section>

<style>
  .detail {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 24px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .hero {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .title {
    min-width: 0;
  }

  h1 {
    margin: 4px 0 10px;
    font-size: 22px;
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
    flex-direction: column;
    gap: 4px;
  }

  .actions button {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 34px;
    text-align: left;
  }

  .actions button:not(.primary) {
    background: none;
    border-color: transparent;
    color: var(--text-dim);
  }

  .actions button:not(.primary):hover:not(:disabled) {
    background: var(--bg-panel);
    color: var(--text);
  }

  .actions hr {
    width: 100%;
    margin: 6px 0;
    border: none;
    border-top: 1px solid var(--border);
  }

  /* The one filled control in the window: Play is the reason it exists. */
  .launch {
    justify-content: center;
    min-height: 40px;
    font-size: 15px;
  }

  .actions button.destructive:not(.primary):hover:not(:disabled) {
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
    grid-template-columns: repeat(2, 1fr);
    gap: 1px;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--border);
    overflow: hidden;
  }

  .stats div {
    padding: 12px 14px;
    background: var(--bg);
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
</style>
