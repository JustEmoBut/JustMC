<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, errorMessage, type Account, type DeviceCode } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    accounts,
    onclose,
    embedded = false,
    onchange,
  }: {
    accounts: Account[];
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
    onchange: () => Promise<void>;
  } = $props();

  let offlineName = $state("");
  let pending = $state<DeviceCode | null>(null);
  let waiting = $state(false);

  async function addOffline() {
    try {
      await api.addOfflineAccount(offlineName);
      offlineName = "";
      await onchange();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  async function signIn() {
    try {
      pending = await api.beginMicrosoftLogin();
      await openUrl(pending.verification_uri);
      waiting = true;
      // Resolves only once the user finishes in the browser, so the dialog
      // stays open showing the code until then.
      const account = await api.completeMicrosoftLogin(pending);
      notify(`Signed in as ${account.name}`);
      await onchange();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      pending = null;
      waiting = false;
    }
  }

  async function remove(id: string) {
    try {
      await api.removeAccount(id);
      await onchange();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  function copyCode() {
    if (pending) navigator.clipboard.writeText(pending.user_code);
  }
</script>

<Modal title="Accounts" {onclose} {embedded} width="480px">
  {#if accounts.length}
    <ul class="list">
      {#each accounts as account (account.id)}
        <li>
          <img
            src="https://api.mineatar.io/face/{account.id}?scale=8"
            alt=""
            width="32"
            height="32"
          />
          <div class="who">
            <strong>{account.name}</strong>
            <span class="faint">{account.kind === "microsoft" ? "Microsoft" : "Offline"}</span>
          </div>
          <button class="danger" onclick={() => remove(account.id)}>Remove</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted empty">No accounts yet.</p>
  {/if}

  {#if pending}
    <div class="code-box">
      <p class="muted">Enter this code in the browser window that just opened:</p>
      <button class="code" onclick={copyCode} title="Click to copy">{pending.user_code}</button>
      <p class="faint">
        {waiting ? "Waiting for you to finish signing in…" : "Opening browser…"}
      </p>
    </div>
  {/if}

  <hr />

  <div class="field">
    <label for="offline-name">Add an offline account</label>
    <div class="row">
      <input
        id="offline-name"
        bind:value={offlineName}
        placeholder="Username"
        maxlength="16"
        onkeydown={(e) => e.key === "Enter" && addOffline()}
      />
      <button onclick={addOffline} disabled={!offlineName.trim()}>Add</button>
    </div>
    <p class="faint">Offline accounts cannot join online servers.</p>
  </div>

  {#snippet footer()}
    <button onclick={onclose}>Close</button>
    <button class="primary" onclick={signIn} disabled={!!pending}>
      Sign in with Microsoft
    </button>
  {/snippet}
</Modal>

<style>
  .list {
    list-style: none;
    margin: 0 0 4px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .list li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    background: var(--bg-inset);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .list img {
    border-radius: 4px;
    image-rendering: pixelated;
    background: var(--border);
  }

  .who {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .empty {
    text-align: center;
    padding: 8px 0 4px;
  }

  hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 18px 0;
  }

  .code-box {
    margin-top: 14px;
    padding: 14px;
    background: var(--bg-inset);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    text-align: center;
  }

  .code-box p {
    margin: 0;
  }

  .code {
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 26px;
    letter-spacing: 5px;
    font-weight: 600;
    color: var(--accent);
    background: transparent;
    border: none;
    padding: 10px 0 6px;
    width: 100%;
  }
</style>
