<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ModProject } from "./api";
  import { count, label } from "./format";
  import { renderMarkdown } from "./markdown";

  let {
    project,
    builds,
    children,
  }: {
    project: ModProject;
    /** How many builds fit this instance (or exist, for a pack). */
    builds: number;
    /** The install controls, between the summary and the description. */
    children: Snippet;
  } = $props();
</script>

<!-- One catalogue project, as the mod browser and the modpack browser both
     show it. -->
<div class="sheet-head">
  {#if project.icon_url}
    <img src={project.icon_url} alt="" width="52" height="52" />
  {:else}
    <span class="slot big" aria-hidden="true">{project.title.slice(0, 1)}</span>
  {/if}
  <div>
    <h3>{project.title}</h3>
    <p class="cats">
      {#each project.categories.slice(0, 4) as c (c)}
        <span class="chip">{label(c)}</span>
      {/each}
    </p>
  </div>
</div>

<dl class="stats data">
  <div><dt>Downloads</dt><dd>{count(project.downloads)}</dd></div>
  <div><dt>Followers</dt><dd>{count(project.followers)}</dd></div>
  <div><dt>Builds</dt><dd>{builds}</dd></div>
</dl>

<p class="lede">{project.description}</p>

{@render children()}

<!-- Author-written markdown, parsed and then sanitised; see markdown.ts for
     what is allowed through. -->
<div class="body md">{@html renderMarkdown(project.body)}</div>

<style>
  /* Every mod gets a slot, filled with its initial when it has no icon, so the
     rows line up on a single left edge whether artwork loaded or not. */
  .slot {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: none;
    border-radius: var(--radius-sm);
    background: var(--bg-inset);
    box-shadow: var(--well);
    color: var(--text-faint);
    font-size: 14px;
    font-weight: 700;
  }

  .slot.big {
    width: 52px;
    height: 52px;
    font-size: 21px;
  }

  .sheet-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .sheet-head img {
    border-radius: var(--radius-sm);
    box-shadow: var(--well), 0 0 0 1px rgb(0 0 0 / 0.4);
    background: var(--bg-inset);
    flex: none;
  }

  h3 {
    font-size: 17px;
  }

  .cats {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin: 7px 0 0;
  }

  .chip {
    padding: 2px 8px;
    border-radius: 100px;
    background: rgb(255 255 255 / 0.055);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.05);
    font-size: 10.5px;
    font-weight: 600;
    font-stretch: 82%;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
  }

  /* Three facts, set as a readout rather than a sentence, because they are the
     numbers people actually compare mods on. */
  .stats {
    display: flex;
    gap: 2px;
    margin: 16px 0;
    padding: 0;
    border-radius: var(--radius);
    overflow: hidden;
    background: rgb(0 0 0 / 0.25);
    box-shadow: var(--well);
  }

  .stats div {
    flex: 1;
    padding: 9px 12px;
  }

  .stats dt {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--text-faint);
  }

  .stats dd {
    margin: 3px 0 0;
    font-size: 15px;
    color: var(--text);
  }

  .lede {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-dim);
  }

  .body {
    margin-top: 4px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--text-dim);
    overflow-wrap: anywhere;
  }

  /* Project descriptions are full documents: headings, tables, screenshots,
     badge rows. They get a real type scale, one step below the app's own, so a
     mod's own hierarchy reads without competing with the window's. */
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 18px 0 8px;
    font-size: 14px;
    font-weight: 700;
    color: var(--text);
  }

  .md :global(h1) {
    font-size: 16px;
  }

  .md :global(h1:first-child),
  .md :global(h2:first-child),
  .md :global(h3:first-child) {
    margin-top: 0;
  }

  .md :global(p) {
    margin: 0 0 10px;
  }

  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 10px;
    padding-left: 20px;
  }

  .md :global(li) {
    display: list-item;
    padding: 0;
    border: 0;
    margin-bottom: 3px;
  }

  .md :global(a) {
    color: var(--accent-lit);
    text-decoration: none;
  }

  .md :global(a:hover) {
    text-decoration: underline;
  }

  .md :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-sm);
    margin: 4px 0;
    vertical-align: middle;
  }

  .md :global(code) {
    padding: 1px 5px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.35);
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text);
  }

  .md :global(pre) {
    max-width: 100%;
    margin: 0 0 10px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-inset);
    box-shadow: var(--well);
    overflow-x: auto;
  }

  .md :global(pre code) {
    padding: 0;
    background: none;
  }

  .md :global(blockquote) {
    margin: 0 0 10px;
    padding-left: 12px;
    border-left: 2px solid var(--border-strong);
    color: var(--text-faint);
  }

  .md :global(hr) {
    margin: 16px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  .md :global(table) {
    width: 100%;
    max-width: 100%;
    display: block;
    overflow-x: auto;
    margin-bottom: 10px;
    border-collapse: collapse;
    font-size: 11.5px;
  }

  .md :global(th),
  .md :global(td) {
    padding: 5px 8px;
    border: 1px solid var(--border);
    text-align: left;
  }

  .md :global(th) {
    background: rgb(255 255 255 / 0.04);
    color: var(--text);
  }
</style>
