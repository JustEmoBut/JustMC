<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  export type Page = { id: string; label: string; icon: IconName };
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    title,
    pages,
    page = $bindable(),
    onclose,
    content,
  }: {
    title: string;
    pages: Page[];
    page: string;
    onclose: () => void;
    /** Renders the page whose id it is handed. */
    content: Snippet<[string]>;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<!-- Prism's paged window: the list of pages on the left, the open one beside
     it. Like `Modal`, the backdrop does not close it. -->
<div class="backdrop" role="presentation">
  <div class="window" role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="ghost" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <div class="split">
      <nav aria-label="Pages">
        {#each pages as p (p.id)}
          <button class:active={page === p.id} aria-current={page === p.id} onclick={() => (page = p.id)}>
            <Icon name={p.icon} size={16} />
            {p.label}
          </button>
        {/each}
      </nav>
      <div class="content">
        {#key page}
          {@render content(page)}
        {/key}
      </div>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.6);
    backdrop-filter: blur(3px);
    display: grid;
    place-items: center;
    z-index: 50;
    animation: fade 0.12s ease-out;
  }

  .window {
    /* The starting size, not a cap: dragged with the browser's own handle,
       the same as `Modal`, and kept as long as the window stays open. */
    width: min(1100px, calc(100vw - 40px));
    height: min(760px, calc(100vh - 40px));
    min-width: 560px;
    min-height: 360px;
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
    resize: both;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
    animation: rise 0.18s var(--ease);
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 12px 12px 20px;
    border-bottom: 1px solid var(--border);
  }

  header h2 {
    flex: 1;
    font-size: 15px;
    font-weight: 650;
  }

  .split {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  nav {
    width: 190px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 8px;
    background: var(--bg-inset);
    border-right: 1px solid var(--border);
    overflow-y: auto;
    overflow-x: hidden;
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    text-align: left;
    background: none;
    border-color: transparent;
    color: var(--text-dim);
  }

  nav button:hover:not(.active) {
    background: var(--bg-panel);
  }

  nav button.active {
    background: var(--accent-soft);
    color: var(--accent-lit);
  }

  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  @keyframes fade {
    from { opacity: 0; }
  }

  @keyframes rise {
    from { opacity: 0; transform: translateY(6px) scale(0.985); }
  }
</style>
