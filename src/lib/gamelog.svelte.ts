/**
 * The live output of the game this session launched. `App.svelte` subscribes to
 * `game-log` once and appends here; the log page of whichever instance window
 * is open reads it.
 */
export const gameLog = $state({ lines: [] as string[] });

/** A long session emits tens of thousands of lines; rendering all of them is
    what makes launcher log views crawl. */
const MAX_LINES = 2000;

export function appendLine(line: string) {
  gameLog.lines = [...gameLog.lines.slice(-MAX_LINES), line];
}

export function clearLog() {
  gameLog.lines = [];
}
