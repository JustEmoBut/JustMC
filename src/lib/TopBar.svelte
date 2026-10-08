<script lang="ts">
  import type { Account } from "./api";
  import Avatar from "./Avatar.svelte";
  import Icon from "./Icon.svelte";

  let {
    search = $bindable(),
    account,
    onnew,
    onfolders,
    onsettings,
    onaccounts,
  }: {
    search: string;
    account: Account | undefined;
    onnew: () => void;
    onfolders: () => void;
    onsettings: () => void;
    onaccounts: () => void;
  } = $props();
</script>

<!-- Prism's main toolbar: the launcher-wide actions on the left, who you are
     on the right. -->
<header class="topbar">
  <div class="brand">
    <div class="mark" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
    <strong>JustLauncher</strong>
  </div>

  <button class="ghost tool" onclick={onnew}>
    <Icon name="plus" size={14} />
    Add Instance
  </button>
  <button class="ghost tool" onclick={onfolders} title="Exports folder">
    <Icon name="folder" size={14} />
    Folders
  </button>
  <button class="ghost tool" onclick={onsettings}>
    <Icon name="gear" size={14} />
    Settings
  </button>

  <span class="spacer"></span>

  <div class="find">
    <Icon name="search" size={14} />
    <input bind:value={search} placeholder="Search instances" aria-label="Search instances" />
  </div>

  <button class="ghost account" onclick={onaccounts} title="Accounts">
    {#if account}
      <Avatar url={account.skin_url} size={24} />
    {:else}
      <Icon name="user" />
    {/if}
    <span>{account ? account.name : "Add account"}</span>
  </button>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 52px;
    flex: none;
    padding: 0 12px 0 16px;
    background: var(--bg-raised);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-right: 18px;
    font-size: 15px;
    white-space: nowrap;
  }

  /* The mark is a 2x2 block face, the same shape the instance icons use --
     the app signs itself with its own vocabulary rather than a logotype. */
  .mark {
    display: grid;
    grid-template: repeat(2, 7px) / repeat(2, 7px);
    gap: 2px;
    transform: rotate(45deg);
    margin: 0 4px;
  }

  .mark span {
    border-radius: 1.5px;
    background: var(--accent);
  }

  .mark span:nth-child(2) {
    background: var(--accent-lit);
  }

  .mark span:nth-child(4) {
    opacity: 0.55;
  }

  .tool {
    display: flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }

  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    width: min(280px, 28vw);
    padding: 0 10px;
    background: var(--bg-inset);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-faint);
    transition: border-color 0.15s var(--ease);
  }

  .find:focus-within {
    border-color: var(--accent);
    color: var(--text-dim);
  }

  .find input {
    border: none;
    background: none;
    padding: 7px 0;
    font-size: 13px;
  }

  .find input:focus {
    outline: none;
  }

  .account {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 180px;
    margin-left: 6px;
    padding: 5px 10px 5px 6px;
    color: var(--text);
    border: 1px solid var(--border);
  }

  .account span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 960px) {
    .tool,
    .account span {
      font-size: 0;
      gap: 0;
    }
  }
</style>
