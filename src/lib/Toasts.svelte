<script lang="ts">
  import { toasts, dismiss } from "./toast.svelte";
</script>

<div class="stack">
  {#each toasts.items as toast (toast.id)}
    <button class="toast {toast.kind}" onclick={() => dismiss(toast.id)}>
      {toast.text}
    </button>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 18px;
    bottom: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 80;
    max-width: 420px;
  }

  .toast {
    text-align: left;
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-left: 3px solid var(--text-faint);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow);
    padding: 10px 14px;
    font-size: 13px;
    line-height: 1.45;
    animation: slide 0.15s ease-out;
    user-select: text;
  }

  .toast.error {
    border-left-color: var(--danger);
  }

  .toast.info {
    border-left-color: var(--accent);
  }

  @keyframes slide {
    from { opacity: 0; transform: translateX(12px); }
  }
</style>
