<script lang="ts">
  import { task } from "./task.svelte";

  const t = $derived(task.current);

  /** Bytes make a truer bar than file counts: assets are tiny, the jar is 40 MB. */
  function fraction() {
    const p = t?.progress;
    if (!p) return 0;
    if (p.bytes_total) return p.bytes_done / p.bytes_total;
    if (p.files_total) return p.files_done / p.files_total;
    return 0;
  }

  function bytes(n: number) {
    if (n < 1024) return `${n} B`;
    const units = ["KB", "MB", "GB"];
    let value = n / 1024;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
  }

  function eta() {
    const p = t?.progress;
    if (!p?.bytes_per_sec || !p.bytes_total) return null;
    const seconds = Math.round(Math.max(0, p.bytes_total - p.bytes_done) / p.bytes_per_sec);
    if (seconds < 60) return `${seconds}s left`;
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s left`;
  }
</script>

{#if t}
  <!-- Not dismissable: the work carries on either way, and a window the user
       can close halfway through only hides what is happening. -->
  <div class="backdrop" role="presentation">
    <div class="box" role="status" aria-live="polite">
      <p class="eyebrow">{t.total > 1 ? `${t.index + 1} of ${t.total}` : "Working"}</p>
      <h2>{t.title}</h2>
      {#if t.detail}
        <p class="detail">{t.detail}</p>
      {/if}

      <div class="bar"><span style:width="{fraction() * 100}%"></span></div>

      <div class="readout data">
        {#if t.progress}
          <span class="stage">{t.progress.stage}</span>
          <span>{t.progress.files_done}/{t.progress.files_total} files</span>
          <span class="spacer"></span>
          {#if t.progress.bytes_per_sec}
            <span class="rate">{bytes(t.progress.bytes_per_sec)}/s</span>
          {/if}
          {#if eta()}
            <span>{eta()}</span>
          {/if}
        {:else}
          <span>Preparing…</span>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    background: rgb(6 8 10 / 0.72);
    backdrop-filter: blur(3px);
    animation: fade 0.12s ease-out;
  }

  .box {
    width: 420px;
    max-width: calc(100vw - 40px);
    padding: 22px 24px 20px;
    border: 1px solid #0c0e11;
    border-radius: var(--radius);
    background: linear-gradient(var(--bg-panel), var(--bg-raised) 140px);
    box-shadow: var(--shadow);
    animation: rise 0.14s ease-out;
  }

  h2 {
    margin-top: 6px;
    font-size: 17px;
  }

  .detail {
    margin: 6px 0 0;
    font-size: 13px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The same durability bar as the status strip, given room to be the subject
     of the window rather than a hairline at the bottom of it. */
  .bar {
    height: 8px;
    margin: 18px 0 10px;
    border-radius: 100px;
    background: var(--bg-inset);
    box-shadow: var(--well);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-lit));
    box-shadow: 0 0 12px rgb(88 207 149 / 0.5);
    transition: width 0.2s ease-out;
  }

  .readout {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-faint);
  }

  .stage {
    color: var(--text-dim);
    font-weight: 600;
  }

  .rate {
    color: var(--accent-lit);
    font-weight: 600;
  }

  .spacer {
    flex: 1;
  }

  @keyframes fade {
    from { opacity: 0; }
  }

  @keyframes rise {
    from { opacity: 0; transform: translateY(8px); }
  }
</style>
