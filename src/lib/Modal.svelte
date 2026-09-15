<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    width = "460px",
    children,
    footer,
  }: {
    title: string;
    onclose: () => void;
    width?: string;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

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

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(6 8 10 / 0.68);
    backdrop-filter: blur(3px);
    display: grid;
    place-items: center;
    z-index: 50;
    animation: fade 0.12s ease-out;
  }

  .dialog {
    background: linear-gradient(var(--bg-panel), var(--bg-raised) 140px);
    border: 1px solid #0c0e11;
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    max-width: calc(100vw - 40px);
    max-height: calc(100vh - 60px);
    display: flex;
    flex-direction: column;
    animation: rise 0.14s ease-out;
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
    padding: 14px 14px 14px 18px;
    border-bottom: 1px solid #0c0e11;
    box-shadow: 0 1px 0 rgb(255 255 255 / 0.035);
  }

  header h2 {
    font-size: 15px;
    flex: 1;
  }

  .body {
    padding: 18px;
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
    padding: 14px 18px;
    border-top: 1px solid #0c0e11;
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.035);
  }

  @keyframes fade {
    from { opacity: 0; }
  }

  @keyframes rise {
    from { opacity: 0; transform: translateY(8px); }
  }
</style>
