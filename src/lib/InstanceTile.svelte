<script lang="ts">
  import { loaderName, type Instance } from "./api";
  import BlockIcon from "./BlockIcon.svelte";

  let {
    instance,
    selected,
    running,
    onselect,
    onlaunch,
    onmenu,
  }: {
    instance: Instance;
    selected: boolean;
    running: boolean;
    onselect: (instance: Instance) => void;
    onlaunch: (instance: Instance) => void;
    onmenu: (instance: Instance, event: MouseEvent) => void;
  } = $props();
</script>

<div
  class="tile"
  class:selected
  class:running
  role="option"
  aria-selected={selected}
  tabindex="0"
  title="{instance.name} — {instance.mc_version}"
  onclick={() => onselect(instance)}
  oncontextmenu={(e) => {
    onselect(instance);
    onmenu(instance, e);
  }}
  ondblclick={() => onlaunch(instance)}
  onkeydown={(e) => {
    if (e.key === "Enter") onlaunch(instance);
    else if (e.key === " ") {
      e.preventDefault();
      onselect(instance);
    }
  }}
>
  <BlockIcon name={instance.name} size={32} fabric={instance.loader !== "vanilla"} />
  <span class="text">
    <span class="name">{instance.name}</span>
    <span class="version">{instance.mc_version} · {loaderName(instance.loader)}</span>
  </span>
  {#if running}
    <span class="live" aria-label="Running"></span>
  {/if}
</div>

<style>
  /* A list row in the sidebar: the icon is the landmark, the name the label,
     and the version a quiet second line. */
  .tile {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 7px 10px;
    border-radius: var(--radius);
    cursor: pointer;
    transition: background 0.15s var(--ease);
  }

  .tile:hover {
    background: var(--bg-hover);
  }

  .tile.selected {
    background: var(--bg-panel);
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .tile:focus-visible {
    outline: 2px solid rgb(34 197 94 / 0.45);
    outline-offset: -2px;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .name,
  .version {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: 13.5px;
    font-weight: 550;
  }

  .version {
    font-size: 12px;
    color: var(--text-faint);
  }

  .selected .version {
    color: var(--text-dim);
  }

  /* The only animation in the list, spent on the one thing that is live. */
  .live {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
    animation: breathe 2.4s ease-in-out infinite;
  }

  @keyframes breathe {
    50% {
      box-shadow: 0 0 0 6px transparent;
    }
  }
</style>
