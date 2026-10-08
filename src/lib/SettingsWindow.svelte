<script lang="ts">
  import Accounts from "./Accounts.svelte";
  import type { Account, Settings as SettingsData } from "./api";
  import PageWindow, { type Page } from "./PageWindow.svelte";
  import Settings from "./Settings.svelte";
  import StorageSweep from "./StorageSweep.svelte";

  let {
    settings,
    accounts,
    page = $bindable("launcher"),
    onclose,
    onsaved,
    onaccounts,
  }: {
    settings: SettingsData;
    accounts: Account[];
    page?: string;
    onclose: () => void;
    onsaved: (settings: SettingsData) => void;
    onaccounts: () => Promise<void>;
  } = $props();

  const pages: Page[] = [
    { id: "launcher", label: "Launcher", icon: "gear" },
    { id: "accounts", label: "Accounts", icon: "user" },
    { id: "storage", label: "Storage", icon: "folder" },
  ];
</script>

<PageWindow title="Settings" {pages} bind:page {onclose}>
  {#snippet content(id: string)}
    {#if id === "launcher"}
      <Settings {settings} {onclose} {onsaved} embedded />
    {:else if id === "accounts"}
      <Accounts {accounts} {onclose} onchange={onaccounts} embedded />
    {:else if id === "storage"}
      <StorageSweep />
    {/if}
  {/snippet}
</PageWindow>
