<script lang="ts">
  /**
   * A small stroke icon set, inline rather than from a library: the app needs
   * a handful of glyphs and an icon package would be a dependency and a bundle for it.
   */
  export type IconName =
    | "play"
    | "stop"
    | "plus"
    | "import"
    | "export"
    | "folder"
    | "sliders"
    | "trash"
    | "user"
    | "search"
    | "copy"
    | "gear";

  let { name, size = 15 }: { name: IconName; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.4"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "play"}
    <path d="M4.5 3.2 12.5 8l-8 4.8z" fill="currentColor" stroke-width="1" />
  {:else if name === "stop"}
    <rect x="4" y="4" width="8" height="8" rx="1" fill="currentColor" stroke-width="1" />
  {:else if name === "plus"}
    <path d="M8 3.5v9M3.5 8h9" />
  {:else if name === "import"}
    <path d="M8 2.5v7M5 7l3 3 3-3M3 12.5h10" />
  {:else if name === "export"}
    <path d="M8 10.5v-7M5 6l3-3 3 3M3 12.5h10" />
  {:else if name === "folder"}
    <path d="M2 4.5h4l1.2 1.5H14v6.5H2z" />
  {:else if name === "sliders"}
    <path d="M2.5 4.5h11M2.5 11.5h11" />
    <circle cx="6" cy="4.5" r="1.7" fill="var(--bg-raised)" />
    <circle cx="10.5" cy="11.5" r="1.7" fill="var(--bg-raised)" />
  {:else if name === "trash"}
    <path d="M2.8 4.2h10.4M6.4 4.2V2.8h3.2v1.4M4.2 4.2l.6 9h6.4l.6-9M6.7 6.6v4.2M9.3 6.6v4.2" />
  {:else if name === "user"}
    <circle cx="8" cy="5.6" r="2.5" />
    <path d="M3 13.4c0-2.4 2.2-3.8 5-3.8s5 1.4 5 3.8" />
  {:else if name === "gear"}
    <circle cx="8" cy="8" r="2.2" />
    <path
      d="M8 1.8v1.6M8 12.6v1.6M2.6 8H1M15 8h-1.6M4.2 4.2 3 3M13 13l-1.2-1.2M11.8 4.2 13 3M3 13l1.2-1.2"
    />
  {:else if name === "copy"}
    <rect x="5.5" y="5.5" width="8" height="8" rx="1.2" />
    <path d="M10.5 3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1" />
  {:else if name === "search"}
    <circle cx="7.2" cy="7.2" r="4.2" />
    <path d="m10.4 10.4 3 3" />
  {/if}
</svg>

<style>
  svg {
    flex: none;
    display: block;
  }
</style>
