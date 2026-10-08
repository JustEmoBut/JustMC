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
  // In place: copying the whole buffer per line is what a chatty mod pays for.
  gameLog.lines.push(line);
  if (gameLog.lines.length > MAX_LINES) {
    gameLog.lines.splice(0, gameLog.lines.length - MAX_LINES);
  }
}

export function clearLog() {
  gameLog.lines = [];
}
