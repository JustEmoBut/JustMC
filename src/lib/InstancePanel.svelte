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

<aside>
  <div class="identity">
    <BlockIcon
      name={instance.name}
      size={56}
      fabric={instance.loader !== "vanilla"}
    />
    <h2>{instance.name}</h2>
    <p class="data">
      {instance.mc_version} · {loaderName(instance.loader)}
    </p>
  </div>

  <button class="launch" disabled={!!status} onclick={() => onlaunch(instance)}>
    <Icon name="play" size={13} />
    {status ?? "Launch"}
  </button>

  {#if status === "Running"}
    <div class="row running">
      <button class="stop" onclick={() => actions.stopInstance(instance)}>
        <Icon name="stop" size={13} />
        Stop
      </button>
      <button
        class="stop"
        onclick={() => onrestart(instance)}
        title="Quit the game and start it again"
      >
        <Icon name="refresh" size={13} />
        Restart
      </button>
    </div>
  {/if}

  <!-- The panels an instance holds: a grid of tiles rather than ten identical
       rows, so the eye picks one out by its icon instead of reading a list. -->
  <div class="tiles">
    <button onclick={() => onmods(instance)}>
      <Icon name="package" size={18} />
      Content
    </button>
    <button onclick={() => onworlds(instance)}>
      <Icon name="globe" size={18} />
      Worlds
    </button>
    <button onclick={() => onservers(instance)}>
      <Icon name="server" size={18} />
      Servers
    </button>
    <button onclick={() => onoptions(instance)} title="Edit the game's options.txt">
      <Icon name="gear" size={18} />
      Options
    </button>
    <button onclick={() => onscreenshots(instance)}>
      <Icon name="image" size={18} />
      Screenshots
    </button>
    <button onclick={() => actions.openFolder(instance)}>
      <Icon name="folder" size={18} />
      Folder
    </button>
  </div>

  <!-- What is done *to* the instance rather than opened inside it. -->
  <nav>
    <button
      disabled={!!status}
      title={status ? "Stop the game before editing this instance." : undefined}
      onclick={() => onedit(instance)}
    >
      <Icon name="sliders" />
      Edit
    </button>
    <button onclick={duplicate} disabled={duplicating || !!status}>
      <Icon name="copy" />
      {duplicating ? "Copying…" : "Duplicate"}
    </button>
    <button onclick={() => onexport(instance)}>
      <Icon name="export" />
      Export
    </button>

    {#if confirmingDelete}
      <div class="confirm">
        <p>Delete this instance and every world in it?</p>
        <div class="row">
          <button onclick={() => (confirmingDelete = false)}>Cancel</button>
          <button class="really" onclick={remove}>Delete</button>
        </div>
      </div>
    {:else}
      <button
        class="destructive"
        disabled={!!status}
        title={status ? "Stop the game before deleting this instance." : undefined}
        onclick={() => (confirmingDelete = true)}
      >
        <Icon name="trash" />
        Delete
      </button>
    {/if}
  </nav>

  <p class="footnote">
    {played(instance.last_played)}{#if playTime(instance.play_time)} · {playTime(instance.play_time)}{/if}
  </p>
</aside>

<style>
  aside {
    width: 244px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 22px 16px 16px;
    background: linear-gradient(var(--bg-raised), #171a1f 220px);
    border-left: 1px solid #0c0e11;
    box-shadow: inset 1px 0 0 rgb(255 255 255 / 0.035);
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* The block sits in a lit alcove: the selected instance gets the one pool of
     colour in the window, so the eye knows what the column is about. */
  .identity {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 18px 10px 16px;
    border-radius: var(--radius);
    background: radial-gradient(120% 90% at 50% 0%, rgb(63 178 122 / 0.16), transparent 72%);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.04);
    text-align: center;
  }

  h2 {
    font-size: 15.5px;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }

  .identity p {
    margin: 0;
    color: var(--text-faint);
  }

  /* The one filled control in the window: everything else in this column is a
     list item, and Launch is the reason the column exists. */
  .launch {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 9px;
    padding: 12px;
    font-size: 14px;
    font-weight: 700;
    font-stretch: 90%;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    background: linear-gradient(var(--accent-lit), var(--accent));
    border-color: #2f8d5f;
    color: var(--accent-ink);
    box-shadow: var(--bevel), 0 0 24px rgb(63 178 122 / 0.25);
  }

  .running {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }

  .stop {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 7px;
  }

  .launch:hover:not(:disabled) {
    background: linear-gradient(#6ee0a6, var(--accent-lit));
    border-color: var(--accent-lit);
  }

  /* Two columns of square-ish tiles: six destinations fit in three rows, and
     an icon above its label is what makes them tellable apart at a glance. */
  .tiles {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .tiles button {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 12px 6px;
    min-height: 68px;
    font-size: 11.5px;
    color: var(--text-dim);
    background: rgb(255 255 255 / 0.03);
    box-shadow: none;
    border-color: rgb(255 255 255 / 0.05);
    text-align: center;
    overflow-wrap: anywhere;
  }

  .tiles button:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.08);
    border-color: rgb(255 255 255 / 0.12);
    color: var(--text);
  }

  /* A vertical action list, the way a desktop tool presents object actions:
     left-aligned, icon then verb, no boxes competing for attention. The tiles
     above open a panel; these change the instance itself, and the rule between
     them is the whole point of the split. */
  nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-top: 12px;
    border-top: 1px solid rgb(255 255 255 / 0.06);
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 8px 10px;
    background: none;
    box-shadow: none;
    border-color: transparent;
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    font-size: 13px;
    text-align: left;
  }

  nav button:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.06);
    border-color: transparent;
    color: var(--text);
  }

  nav button.destructive:hover:not(:disabled) {
    background: rgb(217 112 95 / 0.14);
    color: var(--danger);
  }

  .confirm {
    padding: 9px;
    border: 1px solid rgb(217 112 95 / 0.4);
    border-radius: 3px;
    background: rgb(217 112 95 / 0.08);
  }

  .confirm p {
    margin: 0 0 9px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-dim);
  }

  .confirm .row {
    gap: 6px;
  }

  .confirm button {
    flex: 1;
    padding: 5px;
    font-size: 12px;
  }

  .really {
    background: var(--danger);
    border-color: var(--danger);
    color: #2a0f0b;
    font-weight: 600;
  }

  /* Pinned to the bottom so the action list stays tight to the launch button
     instead of floating in the middle of the column. */
  .footnote {
    margin: auto 0 0;
    font-size: 11.5px;
    color: var(--text-faint);
    text-align: center;
  }
</style>
