<script lang="ts">
  import type { Instance } from "./api";
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
  <BlockIcon name={instance.name} size={52} fabric={instance.loader !== "vanilla"} />
  <span class="name">{instance.name}</span>
  <span class="version">{instance.mc_version}</span>
</div>

<style>
  /* An inventory slot, not a card. The game this launches puts everything you
     own in a bevelled well on a grid, and that is the one piece of its visual
     language worth borrowing wholesale: it makes a wall of instances scannable
     without a single decorative pixel. */
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 9px;
    width: 118px;
    height: 140px;
    padding: 14px 8px 10px;
    border: 1px solid #0d0f12;
    border-radius: var(--radius);
    background: linear-gradient(var(--bg-raised), #171a1f);
    box-shadow: var(--bevel);
    cursor: pointer;
    text-align: center;
    transition: background 0.12s, border-color 0.12s, transform 0.09s ease-out,
      box-shadow 0.12s;
  }

  /* Hover lifts the whole slot a pixel, the way picking an item up does. */
  .tile:hover {
    transform: translateY(-2px);
    background: linear-gradient(#252a32, #1b1f25);
    box-shadow: var(--bevel), 0 8px 18px rgb(0 0 0 / 0.45);
  }

  .tile.selected {
    border-color: #2f8d5f;
    background: linear-gradient(rgb(63 178 122 / 0.16), rgb(63 178 122 / 0.05));
    box-shadow: var(--bevel), 0 0 0 1px rgb(63 178 122 / 0.35),
      0 8px 22px rgb(0 0 0 / 0.5);
  }

  .tile:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  /* A running instance gets a lit ring and a pulse on the block itself -- the
     only animation in the grid, spent on the one thing that is live. */
  .running :global(.block) {
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 0.35),
      0 0 0 2px var(--accent-lit),
      0 0 16px rgb(88 207 149 / 0.55);
    animation: breathe 2.4s ease-in-out infinite;
  }

  @keyframes breathe {
    50% {
      box-shadow:
        inset 0 0 0 1px rgb(0 0 0 / 0.35),
        0 0 0 2px var(--accent),
        0 0 6px rgb(88 207 149 / 0.3);
    }
  }

  /* Two lines before truncating: "Fabric Sandb…" tells you less than a wrapped
     name does, and the grid has the room. */
  .name {
    font-size: 12.5px;
    line-height: 1.25;
    max-width: 100%;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  .selected .name {
    font-weight: 650;
  }

  .version {
    margin-top: auto;
    padding: 2px 7px;
    border-radius: 100px;
    background: rgb(0 0 0 / 0.35);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.05);
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    color: var(--text-dim);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
