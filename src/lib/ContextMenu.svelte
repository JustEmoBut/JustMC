<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";

  export interface MenuItem {
    label: string;
    icon: IconName;
    danger?: boolean;
    disabled?: boolean;
    action: () => void;
  }

  let {
    x,
    y,
    items,
    onclose,
  }: { x: number; y: number; items: MenuItem[]; onclose: () => void } = $props();

  let menu = $state<HTMLElement | null>(null);

  // Flip the menu back inside the window when it opens near an edge; measured
  // after mount because the size depends on the labels.
  const left = $derived(menu ? Math.min(x, window.innerWidth - menu.offsetWidth - 4) : x);
  const top = $derived(menu ? Math.min(y, window.innerHeight - menu.offsetHeight - 4) : y);

  function run(item: MenuItem) {
    onclose();
    item.action();
  }
</script>

<svelte:window
  onkeydown={(e) => e.key === "Escape" && onclose()}
  onresize={onclose}
  onblur={onclose}
/>

<div class="backdrop" role="presentation" onpointerdown={onclose} oncontextmenu={onclose}></div>

<div bind:this={menu} class="menu" style:left="{left}px" style:top="{top}px" role="menu">
  {#each items as item (item.label)}
    <button
      role="menuitem"
      class:danger={item.danger}
      disabled={item.disabled}
      onclick={() => run(item)}
    >
      <Icon name={item.icon} />
      {item.label}
    </button>
  {/each}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
  }

  .menu {
    position: fixed;
    z-index: 61;
    min-width: 160px;
    padding: 4px;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    animation: rise 0.1s ease-out;
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 6px 10px;
    border: 0;
    border-radius: 3px;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  .menu button:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.07);
  }

  .menu button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .menu button.danger {
    color: var(--danger);
  }

  @keyframes rise {
    from { opacity: 0; transform: translateY(-3px); }
  }
</style>
