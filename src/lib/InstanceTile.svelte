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
  <BlockIcon name={instance.name} size={44} fabric={instance.loader !== "vanilla"} />
  <span class="text">
    <span class="name">{instance.name}</span>
    <span class="version">{instance.mc_version} · {loaderName(instance.loader)}</span>
  </span>
  {#if running}
    <span class="live" aria-label="Running"></span>
  {/if}
</div>

<style>
  /* A card in the library grid: the icon is the landmark, the name the label,
     and the version a quiet second line. */
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    cursor: pointer;
    transition: background 0.15s var(--ease), border-color 0.15s var(--ease);
  }

  .tile:hover {
    background: var(--bg-panel);
    border-color: var(--border-strong);
  }

  .tile.selected {
    background: var(--bg-panel);
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .tile:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: 2px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    width: 100%;
  }

  .name,
  .version {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
  }

  .version {
    font-size: 12px;
    color: var(--text-faint);
  }

  .selected .version {
    color: var(--text-dim);
  }

  /* The only animation in the grid, spent on the one thing that is live. */
  .live {
    position: absolute;
    top: 14px;
    right: 14px;
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
