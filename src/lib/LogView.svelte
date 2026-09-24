<script lang="ts">
  import { redact } from "./redact";
  import type { LogFile } from "./api";

  let {
    lines,
    logs,
    choice,
    onchoose,
    onclose,
  }: {
    lines: string[];
    /** Past logs and crash reports of the selected instance, newest first. */
    logs: LogFile[];
    /** "" is the live output of this session; otherwise "source/file". */
    choice: string;
    onchoose: (choice: string) => void;
    onclose: () => void;
  } = $props();

  function label(log: LogFile) {
    const when = new Date(log.modified * 1000).toLocaleString();
    const what = log.source === "crash" ? "Crash report" : log.file;
    return `${what} — ${when}`;
  }

  async function copy() {
    // Never put the raw log on the clipboard: it contains the session token,
    // and the reason anyone copies a log is to paste it somewhere public.
    await navigator.clipboard.writeText(redact(lines.join("\n")));
    copied = true;
    setTimeout(() => (copied = false), 1800);
  }

  let copied = $state(false);

  let box = $state<HTMLDivElement>();
  let stickToBottom = $state(true);

  // Follow new output unless the user has scrolled up to read something.
  $effect(() => {
    lines.length;
    if (stickToBottom && box) box.scrollTop = box.scrollHeight;
  });

  function onscroll() {
    if (!box) return;
    stickToBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  }

  function severity(line: string): string {
    if (/\b(ERROR|FATAL|Exception|\bat [\w.$]+\()/.test(line)) return "err";
    if (/\bWARN\b/.test(line)) return "warn";
    return "";
  }
</script>

<section class="panel">
  <header>
    <h3>Game output</h3>
    <select
      value={choice}
      onchange={(e) => onchoose(e.currentTarget.value)}
      aria-label="Which log to show"
    >
      <option value="">Live output</option>
      {#each logs as log (log.source + "/" + log.file)}
        <option value="{log.source}/{log.file}">{label(log)}</option>
      {/each}
    </select>
    <span class="faint">{lines.length} lines</span>
    <span class="spacer"></span>
    <button class="ghost" onclick={copy} title="Copies with tokens and usernames removed">
      {copied ? "Copied, redacted" : "Copy"}
    </button>
    <button class="ghost" onclick={onclose} aria-label="Close log">✕</button>
  </header>
  <div class="lines" bind:this={box} {onscroll}>
    {#each lines as line, i (i)}
      <div class={severity(line)}>{line}</div>
    {/each}
  </div>
</section>

<style>
  .panel {
    border-top: 1px solid var(--border);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.035);
    background: var(--bg-raised);
    height: 260px;
    display: flex;
    flex-direction: column;
    flex: none;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px 8px 18px;
    border-bottom: 1px solid var(--border);
    box-shadow: 0 1px 0 rgb(255 255 255 / 0.03);
  }

  header h3 {
    font-size: 13px;
  }

  .lines {
    flex: 1;
    overflow-y: auto;
    padding: 10px 18px 12px;
    background: var(--bg-inset);
    box-shadow: inset 0 2px 4px rgb(0 0 0 / 0.45);
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 12px;
    line-height: 1.55;
    color: var(--text-dim);
    user-select: text;
    white-space: pre-wrap;
    word-break: break-word;
  }

  select {
    max-width: 320px;
  }

  .err {
    color: var(--danger);
  }

  .warn {
    color: #fbbf24;
  }
</style>
