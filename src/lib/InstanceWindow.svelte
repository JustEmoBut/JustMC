<script lang="ts">
  import type { Instance, ModKind, QuickPlay } from "./api";
  import InstanceSettings from "./InstanceSettings.svelte";
  import LogPage from "./LogPage.svelte";
  import Mods from "./Mods.svelte";
  import Options from "./Options.svelte";
  import PageWindow, { type Page } from "./PageWindow.svelte";
  import Screenshots from "./Screenshots.svelte";
  import Servers from "./Servers.svelte";
  import Worlds from "./Worlds.svelte";

  let {
    instance,
    status,
    page = $bindable(),
    changed,
    onclose,
    onplay,
    onsaved,
    onexport,
  }: {
    instance: Instance;
    /** Non-empty while installing or running. */
    status: string | undefined;
    page: string;
    /** Counter the parent bumps when it drops a file into a content folder. */
    changed: number;
    onclose: () => void;
    onplay: (instance: Instance, quickPlay: QuickPlay) => void;
    onsaved: () => Promise<void>;
    onexport: (instance: Instance) => void;
  } = $props();

  const running = $derived(!!status);

  // Mods need a loader; resource packs and shaders work on plain Minecraft.
  const pages = $derived<Page[]>([
    { id: "log", label: "Minecraft log", icon: "terminal" },
    { id: "settings", label: "Settings", icon: "sliders" },
    ...(instance.loader === "vanilla"
      ? []
      : [{ id: "mods", label: "Mods", icon: "puzzle" } as Page]),
    { id: "resourcepacks", label: "Resource packs", icon: "layers" },
    { id: "shaderpacks", label: "Shader packs", icon: "sparkles" },
    { id: "worlds", label: "Worlds", icon: "globe" },
    { id: "servers", label: "Servers", icon: "server" },
    { id: "screenshots", label: "Screenshots", icon: "image" },
    { id: "options", label: "Options", icon: "gear" },
  ]);

  const CONTENT: string[] = ["mods", "resourcepacks", "shaderpacks"];
</script>

<PageWindow title={instance.name} {pages} bind:page {onclose}>
  {#snippet content(id: string)}
    {#if id === "log"}
      <LogPage {instance} />
    {:else if id === "settings"}
      {#if running}
        <p class="notice muted">Stop the game before editing this instance.</p>
      {:else}
        <InstanceSettings {instance} {onclose} {onsaved} {onexport} embedded />
      {/if}
    {:else if CONTENT.includes(id)}
      <Mods {instance} {changed} kind={id as ModKind} {onclose} embedded />
    {:else if id === "worlds"}
      <Worlds
        {instance}
        {running}
        {onclose}
        onplay={(folder) => onplay(instance, { kind: "singleplayer", value: folder })}
        embedded
      />
    {:else if id === "servers"}
      <Servers
        {instance}
        {running}
        {onclose}
        onjoin={(address) => onplay(instance, { kind: "multiplayer", value: address })}
        embedded
      />
    {:else if id === "screenshots"}
      <Screenshots {instance} {onclose} embedded />
    {:else if id === "options"}
      <Options {instance} {running} {onclose} embedded />
    {/if}
  {/snippet}
</PageWindow>

<style>
  .notice {
    padding: 24px;
  }
</style>
