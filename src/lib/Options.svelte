<script lang="ts">
  import { api, errorMessage, type Instance } from "./api";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    running,
    onclose,
    embedded = false,
  }: {
    instance: Instance;
    /** The game rewrites options.txt when it exits, so edits made now are lost. */
    running: boolean;
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
  } = $props();

  let text = $state("");
  let loading = $state(true);
  let saving = $state(false);

  $effect(() => {
    api
      .readOptions(instance.id)
      .then((t) => (text = t))
      .catch((e) => notify(errorMessage(e), "error"))
      .finally(() => (loading = false));
  });

  async function save() {
    saving = true;
    try {
      await api.writeOptions(instance.id, text);
      notify("Saved options.txt.");
      onclose();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    saving = false;
  }
</script>

<Modal title="Options — {instance.name}" {onclose} {embedded} width="720px">
  {#if loading}
    <p class="muted">Reading options.txt…</p>
  {:else}
    {#if running}
      <p class="warn">
        {instance.name} is running. The game rewrites this file when it exits, so
        anything saved now is lost.
      </p>
    {:else if text === ""}
      <p class="muted note">
        This instance has no options.txt yet — the game writes one the first time
        it runs. Anything saved here becomes that file.
      </p>
    {/if}
    <!-- Edited as text on purpose: options.txt is `key:value` per line, and a
         form would have to know all ~150 keys the game keeps adding to. -->
    <textarea bind:value={text} spellcheck="false" disabled={running}></textarea>
  {/if}

  {#snippet footer()}
    <span class="muted">.minecraft/options.txt</span>
    <span class="spacer"></span>
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={loading || saving || running}>
      {saving ? "Saving…" : "Save"}
    </button>
  {/snippet}
</Modal>

<style>
  textarea {
    width: 100%;
    height: 52vh;
    resize: vertical;
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    white-space: pre;
    overflow: auto;
    overflow-x: auto;
  }

  .note,
  .warn {
    margin-bottom: 10px;
    font-size: 12px;
  }

  .warn {
    color: #fbbf24;
  }

  /* In a window page the list takes whatever height is left. */
  :global(.embedded-page) textarea {
    flex: 1;
    min-height: 0;
    height: auto;
    max-height: none;
  }
</style>
