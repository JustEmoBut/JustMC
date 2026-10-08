<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    width = "460px",
    children,
    footer,
    embedded = false,
  }: {
    title: string;
    onclose: () => void;
    width?: string;
    children: Snippet;
    footer?: Snippet;
    /** Rendered as a page of a `PageWindow`: no backdrop, no title bar, and
        Escape is left to the window, which owns the close. */
    embedded?: boolean;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !embedded) onclose();
  }
</script>

<svelte:window {onkeydown} />

{#if embedded}
  <div class="embedded-page" aria-label={title}>
    <div class="body">{@render children()}</div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
{:else}

<!-- The backdrop does not close. A dialog here holds a half-filled form more
     often than not, and a stray click beside it is not a decision to discard
     one; Escape and the close button are. -->
<div class="backdrop" role="presentation">
  <div class="dialog" style:width role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="ghost" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <div class="body">{@render children()}</div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>
{/if}

<style>
  /* Embedded, the page fills the window, so its body is a column a list can
     grow into: `.embedded-page` is what a page's own styles key on to drop
     the fixed heights it uses as a dialog. */
  .embedded-page {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .embedded-page .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .embedded-page footer {
    border-radius: 0;
  }

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

  .dialog {
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    max-width: calc(100vw - 40px);
    max-height: calc(100vh - 60px);
    display: flex;
    flex-direction: column;
    animation: rise 0.18s var(--ease);
    /* `width` is the starting size, not a cap: a mod list or a log is worth
       more room on a large screen. The drag handle is the browser's own, which
       needs a non-visible overflow to appear — .body does the scrolling, so
       clipping here costs nothing. The size lasts as long as the dialog. */
    resize: both;
    overflow: hidden;
    min-width: 320px;
    min-height: 200px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 16px 16px 16px 20px;
    border-bottom: 1px solid var(--border);
  }

  header h2 {
    font-weight: 650;
    font-size: 15px;
    flex: 1;
  }

  .body {
    padding: 20px;
    overflow-y: auto;
    /* overflow-y on its own computes overflow-x as auto, so any content a few
       pixels too wide draws a scrollbar across the bottom of every dialog.
       Content that truly cannot wrap scrolls inside its own box instead. */
    overflow-x: hidden;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    background: var(--bg-inset);
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
    border-top: 1px solid var(--border);
  }

  @keyframes fade {
    from { opacity: 0; }
  }

  @keyframes rise {
    from { opacity: 0; transform: translateY(6px) scale(0.985); }
  }
</style>
