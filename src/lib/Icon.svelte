<script lang="ts" module>
  /**
   * A small stroke icon set, inline rather than from a library: the app needs
   * a handful of glyphs and an icon package would be a dependency and a bundle for it.
   * Drawn on a 24px grid with round caps, so they sit evenly at any size.
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
    | "image"
    | "refresh"
    | "server"
    | "globe"
    | "package"
    | "gear"
    | "terminal"
    | "puzzle"
    | "layers"
    | "sparkles";
</script>

<script lang="ts">
  let { name, size = 15 }: { name: IconName; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.9"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "play"}
    <path d="M7 4.8v14.4a1 1 0 0 0 1.5.86l11.6-7.2a1 1 0 0 0 0-1.72L8.5 3.94A1 1 0 0 0 7 4.8z" fill="currentColor" />
  {:else if name === "stop"}
    <rect x="5.5" y="5.5" width="13" height="13" rx="2.5" fill="currentColor" />
  {:else if name === "plus"}
    <path d="M12 5v14M5 12h14" />
  {:else if name === "image"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <circle cx="9" cy="9.5" r="1.8" />
    <path d="m21 15.5-4.3-4.3a1.5 1.5 0 0 0-2.1 0L5 20.5" />
  {:else if name === "import"}
    <path d="M12 3.5v11M7.5 10l4.5 4.5 4.5-4.5M4.5 15.5v2a2.5 2.5 0 0 0 2.5 2.5h10a2.5 2.5 0 0 0 2.5-2.5v-2" />
  {:else if name === "export"}
    <path d="M12 14.5v-11M7.5 8 12 3.5 16.5 8M4.5 15.5v2a2.5 2.5 0 0 0 2.5 2.5h10a2.5 2.5 0 0 0 2.5-2.5v-2" />
  {:else if name === "folder"}
    <path d="M3.5 7.5A2.5 2.5 0 0 1 6 5h3.4a2 2 0 0 1 1.5.7l1.2 1.3H18a2.5 2.5 0 0 1 2.5 2.5v7.5A2.5 2.5 0 0 1 18 19.5H6a2.5 2.5 0 0 1-2.5-2.5z" />
  {:else if name === "sliders"}
    <path d="M4 7h7M15 7h5M4 17h3M11 17h9" />
    <circle cx="13" cy="7" r="2.2" />
    <circle cx="9" cy="17" r="2.2" />
  {:else if name === "trash"}
    <path d="M4 6.5h16M9.5 6.5V4.8a1.3 1.3 0 0 1 1.3-1.3h2.4a1.3 1.3 0 0 1 1.3 1.3v1.7M6.2 6.5l.9 12.1a2 2 0 0 0 2 1.9h5.8a2 2 0 0 0 2-1.9l.9-12.1M10 10.5v6M14 10.5v6" />
  {:else if name === "user"}
    <circle cx="12" cy="8.5" r="3.8" />
    <path d="M4.5 20c.8-3.5 3.8-5.5 7.5-5.5s6.7 2 7.5 5.5" />
  {:else if name === "gear"}
    <path d="M10.3 3.6a1.8 1.8 0 0 1 3.4 0l.3.9a1.8 1.8 0 0 0 2.5 1l.8-.4a1.8 1.8 0 0 1 2.4 2.4l-.4.8a1.8 1.8 0 0 0 1 2.5l.9.3a1.8 1.8 0 0 1 0 3.4l-.9.3a1.8 1.8 0 0 0-1 2.5l.4.8a1.8 1.8 0 0 1-2.4 2.4l-.8-.4a1.8 1.8 0 0 0-2.5 1l-.3.9a1.8 1.8 0 0 1-3.4 0l-.3-.9a1.8 1.8 0 0 0-2.5-1l-.8.4a1.8 1.8 0 0 1-2.4-2.4l.4-.8a1.8 1.8 0 0 0-1-2.5l-.9-.3a1.8 1.8 0 0 1 0-3.4l.9-.3a1.8 1.8 0 0 0 1-2.5l-.4-.8a1.8 1.8 0 0 1 2.4-2.4l.8.4a1.8 1.8 0 0 0 2.5-1z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === "copy"}
    <rect x="8.5" y="8.5" width="12" height="12" rx="2.5" />
    <path d="M15.5 8.5V6a2.5 2.5 0 0 0-2.5-2.5H6A2.5 2.5 0 0 0 3.5 6v7A2.5 2.5 0 0 0 6 15.5h2.5" />
  {:else if name === "refresh"}
    <path d="M20 12a8 8 0 1 1-2.6-5.9L20 8.5M20 3.5v5h-5" />
  {:else if name === "server"}
    <rect x="3.5" y="4" width="17" height="7" rx="2.2" />
    <rect x="3.5" y="13" width="17" height="7" rx="2.2" />
    <path d="M7.5 7.5h.01M7.5 16.5h.01M11 7.5h5.5M11 16.5h5.5" />
  {:else if name === "globe"}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M3.5 12h17M12 3.5c2.3 2.4 3.4 5.2 3.4 8.5s-1.1 6.1-3.4 8.5c-2.3-2.4-3.4-5.2-3.4-8.5S9.7 5.9 12 3.5z" />
  {:else if name === "package"}
    <path d="M12 3 20 7.4v9.2L12 21l-8-4.4V7.4z" />
    <path d="M4 7.4 12 12l8-4.6M12 12v9M8 5.2l8 4.5" />
  {:else if name === "search"}
    <circle cx="10.8" cy="10.8" r="6.3" />
    <path d="m15.5 15.5 4.5 4.5" />
  {:else if name === "terminal"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="m7.5 9.5 3 2.5-3 2.5M13 15h3.5" />
  {:else if name === "puzzle"}
    <path d="M9.5 4.5a2 2 0 1 1 4 0V6H17a1 1 0 0 1 1 1v3.5h1.5a2 2 0 1 1 0 4H18V18a1 1 0 0 1-1 1h-3.5v-1.5a2 2 0 1 0-4 0V19H6a1 1 0 0 1-1-1v-3.5h1.5a2 2 0 1 0 0-4H5V7a1 1 0 0 1 1-1h3.5z" />
  {:else if name === "layers"}
    <path d="m12 3.5 8.5 4.3L12 12 3.5 7.8z" />
    <path d="m3.5 12 8.5 4.2 8.5-4.2M3.5 16.2 12 20.5l8.5-4.3" />
  {:else if name === "sparkles"}
    <path d="M10 3.5c.5 3.6 2.4 5.5 6 6-3.6.5-5.5 2.4-6 6-.5-3.6-2.4-5.5-6-6 3.6-.5 5.5-2.4 6-6z" />
    <path d="M18 14c.3 1.8 1.2 2.7 3 3-1.8.3-2.7 1.2-3 3-.3-1.8-1.2-2.7-3-3 1.8-.3 2.7-1.2 3-3z" />
  {/if}
</svg>

<style>
  svg {
    flex: none;
    display: block;
  }
</style>
