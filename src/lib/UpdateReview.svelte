<script lang="ts">
  import { untrack } from "svelte";
  import type { ModUpdate } from "./api";
  import { renderMarkdown } from "./markdown";
  import Modal from "./Modal.svelte";

  let {
    updates,
    onclose,
    onconfirm,
  }: {
    updates: ModUpdate[];
    onclose: () => void;
    /** The subset the user kept, in the order shown. */
    onconfirm: (chosen: ModUpdate[]) => void;
  } = $props();

  // Everything is ticked to start with: the user asked to see updates, so the
  // dialog's default answer is "all of them" and the ticks are for opting out.
  // Snapshotted on open, like the instance settings draft — the list the user
  // is deciding about must not shift under them.
  let chosen = $state(untrack(() => new Set(updates.map((u) => u.file))));
  let open = $state(untrack(() => new Set(updates.length === 1 ? [updates[0].file] : [])));

  function toggle(set: Set<string>, key: string) {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  }
</script>

<Modal
  title={updates.length === 1 ? `Update ${updates[0].name}` : `Update ${updates.length} mods`}
  width="640px"
  {onclose}
>
  {#if updates.length > 1}
    <div class="bulk">
      <span class="faint">{chosen.size} of {updates.length} selected</span>
      <span class="spacer"></span>
      <button
        class="ghost"
        disabled={chosen.size === updates.length}
        onclick={() => (chosen = new Set(updates.map((u) => u.file)))}
      >
        Select all
      </button>
      <button class="ghost" disabled={chosen.size === 0} onclick={() => (chosen = new Set())}>
        Select none
      </button>
    </div>
  {/if}

  <ul>
    {#each updates as update (update.file)}
      <li>
        <div class="head">
          <input
            type="checkbox"
            aria-label="Update {update.name}"
            checked={chosen.has(update.file)}
            onchange={() => (chosen = toggle(chosen, update.file))}
          />
          <button class="ghost name" onclick={() => (open = toggle(open, update.file))}>
            <strong>{update.name}</strong>
            <span class="data faint">
              {update.current_version || "installed"} → {update.new_version}
            </span>
          </button>
          <span class="chev" class:down={open.has(update.file)} aria-hidden="true">›</span>
        </div>

        {#if open.has(update.file)}
          <div class="notes md">
            {#if update.changelog}
              {@html renderMarkdown(update.changelog)}
            {:else}
              <p class="faint">This build ships no release notes.</p>
            {/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>

  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button
      class="primary"
      disabled={chosen.size === 0}
      onclick={() => onconfirm(updates.filter((u) => chosen.has(u.file)))}
    >
      Update {chosen.size}
    </button>
  {/snippet}
</Modal>

<style>
  .bulk {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 4px 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }

  .bulk button {
    padding: 3px 8px;
    font-size: 12px;
  }

  ul {
    list-style: none;
    max-height: 56vh;
    overflow-y: auto;
    overflow-x: hidden;
  }

  li {
    border-bottom: 1px solid var(--border);
  }

  li:last-child {
    border-bottom: 0;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 4px;
  }

  .name {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex: 1;
    min-width: 0;
    padding: 4px 6px;
    text-align: left;
  }

  .name span {
    font-size: 11px;
  }

  .chev {
    color: var(--text-faint);
    transition: transform 0.12s;
  }

  .chev.down {
    transform: rotate(90deg);
  }

  /* Release notes are the author's markdown, sanitised like every other body
     in the app. Indented under the mod they belong to. */
  .notes {
    margin: 0 4px 12px 34px;
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    background: var(--bg-inset);
    box-shadow: var(--well);
    font-size: 12px;
    line-height: 1.55;
    color: var(--text-dim);
    max-height: 240px;
    overflow-y: auto;
    /* Setting overflow-y alone makes the browser compute overflow-x as auto
       too, so a few pixels of overhang put a scrollbar across the whole box.
       Only the elements that genuinely cannot wrap get to scroll. */
    overflow-x: hidden;
    overflow-wrap: anywhere;
  }

  .notes :global(h1),
  .notes :global(h2),
  .notes :global(h3),
  .notes :global(h4) {
    margin: 12px 0 6px;
    font-size: 13px;
    color: var(--text);
  }

  .notes :global(h1:first-child),
  .notes :global(h2:first-child),
  .notes :global(h3:first-child) {
    margin-top: 0;
  }

  .notes :global(p) {
    margin: 0 0 8px;
  }

  .notes :global(ul),
  .notes :global(ol) {
    margin: 0 0 8px;
    padding-left: 18px;
  }

  .notes :global(li) {
    display: list-item;
    border: 0;
    margin-bottom: 2px;
  }

  .notes :global(a) {
    color: var(--accent-lit);
    text-decoration: none;
  }

  .notes :global(code) {
    padding: 1px 5px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.35);
    font-family: var(--mono);
    font-size: 11px;
  }

  .notes :global(img) {
    max-width: 100%;
    height: auto;
  }

  .notes :global(pre),
  .notes :global(table) {
    max-width: 100%;
    /* A table only scrolls if it is a block; as a table it would just push the
       box wider and get clipped by the rule above. */
    display: block;
    overflow-x: auto;
  }
</style>
