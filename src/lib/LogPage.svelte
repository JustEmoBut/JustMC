<script lang="ts">
  import { api, errorMessage, type Instance, type LogFile } from "./api";
  import { gameLog } from "./gamelog.svelte";
  import LogView from "./LogView.svelte";
  import { notify } from "./toast.svelte";

  let { instance }: { instance: Instance } = $props();

  /**
   * Past logs and crash reports of this instance. Read when the page opens
   * rather than kept fresh: the folder only changes when a game runs, and
   * re-reading it on every render would be a directory scan per frame.
   */
  let logFiles = $state<LogFile[]>([]);
  /** "" is the live output; otherwise "source/file" out of `logFiles`. */
  let choice = $state("");
  let fileLines = $state<string[]>([]);

  $effect(() => {
    const id = instance.id;
    choice = "";
    fileLines = [];
    api
      .listLogs(id)
      .then((files) => (logFiles = files))
      .catch((e) => notify(errorMessage(e), "error"));
  });

  async function open(next: string) {
    choice = next;
    if (!next) return;
    const [source, ...rest] = next.split("/");
    try {
      fileLines = await api.readLog(instance.id, source as LogFile["source"], rest.join("/"));
    } catch (e) {
      notify(errorMessage(e), "error");
      choice = "";
    }
  }
</script>

<LogView lines={choice ? fileLines : gameLog.lines} logs={logFiles} {choice} onchoose={open} />
