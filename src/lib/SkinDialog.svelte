<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorMessage, type Account, type SkinProfile } from "./api";
  import Avatar from "./Avatar.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    account,
    onclose,
    onchange,
  }: {
    account: Account;
    onclose: () => void;
    /** The stored avatar source may have moved; the account list re-reads it. */
    onchange: () => Promise<void>;
  } = $props();

  let profile = $state<SkinProfile | null>(null);
  let busy = $state(false);
  let variant = $state<"classic" | "slim">("classic");
  let picker = $state<HTMLInputElement>();

  const worn = $derived(profile?.skins.find((s) => s.state.toLowerCase() === "active"));
  const cape = $derived(profile?.capes.find((c) => c.state.toLowerCase() === "active"));

  /** Every change answers with the profile as it now stands. */
  async function run(work: () => Promise<SkinProfile>, done?: string) {
    busy = true;
    try {
      profile = await work();
      if (done) notify(done);
      await onchange();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    busy = false;
  }

  // Once, on open: `onchange` hands down a fresh account object, and a
  // tracked read here would fetch again after every change.
  $effect(() => {
    untrack(() => run(() => api.accountProfile(account.id)));
  });

  // Start from the model the worn skin already uses, so re-uploading a slim
  // skin does not quietly turn it classic.
  $effect(() => {
    if (worn) variant = worn.variant.toLowerCase() === "slim" ? "slim" : "classic";
  });

  /** The file input gives content, never a path, so the bytes cross IPC. */
  async function upload(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    await run(() => api.uploadSkin(account.id, variant, bytes), "Skin changed.");
  }
</script>

<Modal title="Skin — {account.name}" {onclose} width="460px">
  {#if !profile}
    <p class="muted">{busy ? "Reading the profile from Mojang…" : "The profile could not be read."}</p>
  {:else}
    <div class="current">
      <Avatar url={worn?.url ?? ""} size={64} />
      <div class="what">
        <strong>{profile.name}</strong>
        <span class="muted">
          {worn ? `${worn.variant.toLowerCase() === "slim" ? "Slim" : "Classic"} arms` : "Default skin"}
          · {cape ? `${cape.alias || "Cape"} cape` : "no cape"}
        </span>
      </div>
    </div>

    <div class="field">
      <label for="skin-variant">New skin</label>
      <div class="row">
        <select id="skin-variant" bind:value={variant} disabled={busy}>
          <option value="classic">Classic arms (Steve)</option>
          <option value="slim">Slim arms (Alex)</option>
        </select>
        <input bind:this={picker} type="file" accept=".png,image/png" onchange={upload} hidden />
        <button class="primary" disabled={busy} onclick={() => picker?.click()}>Upload PNG…</button>
      </div>
      <p class="faint">A 64×64 PNG, or the older 64×32 layout.</p>
    </div>

    <div class="field">
      <label for="cape">Cape</label>
      {#if profile.capes.length === 0}
        <p class="muted" id="cape">This account owns no capes.</p>
      {:else}
        <select
          id="cape"
          disabled={busy}
          value={cape?.id ?? ""}
          onchange={(e) => {
            const id = e.currentTarget.value;
            run(() => api.setCape(account.id, id || null), id ? "Cape changed." : "Cape hidden.");
          }}
        >
          <option value="">No cape</option>
          {#each profile.capes as c (c.id)}
            <option value={c.id}>{c.alias || c.id}</option>
          {/each}
        </select>
      {/if}
    </div>
  {/if}

  {#snippet footer()}
    <button
      class="danger"
      disabled={busy || !profile}
      onclick={() => run(() => api.resetSkin(account.id), "Skin reset to the default.")}
    >
      Reset skin
    </button>
    <span class="spacer"></span>
    <button onclick={onclose}>Close</button>
  {/snippet}
</Modal>

<style>
  .current {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 18px;
  }

  .what {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .row {
    display: flex;
    gap: 8px;
  }

  .row select {
    flex: 1;
  }

  .field p {
    margin: 6px 0 0;
  }
</style>
