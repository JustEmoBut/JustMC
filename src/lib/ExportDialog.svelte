<script lang="ts">
  import { api, errorMessage, type Instance } from "./api";
  import Modal from "./Modal.svelte";
  import { task } from "./task.svelte";
  import { notify } from "./toast.svelte";

  let { instance, onclose }: { instance: Instance; onclose: () => void } = $props();

  let exporting = $state(false);

  /**
   * Two formats, two audiences: the zip is a backup this launcher reads back,
   * the `.mrpack` is a pack any launcher can install. Both land in the exports
   * folder.
   */
  async function exportAs(kind: "zip" | "mrpack") {
    exporting = true;
    task.begin(`Exporting ${instance.name}`);
    try {
      if (kind === "zip") {
        await api.exportInstance(instance.id);
      } else {
        await api.exportMrpack(instance.id);
      }
      notify(`Exported ${instance.name}.`);
      onclose();
      await api.openExportsFolder();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      task.end();
      exporting = false;
    }
  }
</script>

<Modal title="Export {instance.name}" {onclose} width="440px">
  <div class="choice">
    <button disabled={exporting} onclick={() => exportAs("zip")}>
      <strong>JustLauncher zip</strong>
      <span>
        A full backup — config, worlds, mods, screenshots. Imports straight back
        into JustLauncher.
      </span>
    </button>
    <button disabled={exporting} onclick={() => exportAs("mrpack")}>
      <strong>Modrinth pack (.mrpack)</strong>
      <span>
        For sharing — the mods and configs any launcher can install. Your
        worlds, screenshots, keybinds and server list stay out.
      </span>
    </button>
  </div>
</Modal>

<style>
  .choice {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .choice button {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
    padding: 13px 14px;
    text-align: left;
  }

  .choice span {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-dim);
  }
</style>
