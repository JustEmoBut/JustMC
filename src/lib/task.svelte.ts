/**
 * The one long job running right now, and how far along it is.
 *
 * Downloads report through a single `install-progress` event whoever started
 * them, so the label has to come from the caller: only the caller knows whether
 * this batch is "Installing Sodium" or "Preparing Skyblock". App.svelte
 * subscribes to the event once and renders the window; everything else just
 * calls `begin`/`step`/`end`.
 */
export interface Progress {
  stage: string;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  bytes_per_sec: number;
}

export interface Task {
  /** What the user asked for: "Installing 3 mods". */
  title: string;
  /** The piece being worked on right now: "Sodium". */
  detail: string;
  /** Position in a batch, 0 when there is only one thing to do. */
  index: number;
  total: number;
  progress: Progress | null;
}

let current = $state<Task | null>(null);

export const task = {
  get current() {
    return current;
  },

  begin(title: string, total = 1) {
    current = { title, detail: "", index: 0, total, progress: null };
  },

  /** Move to the next item of a batch. */
  step(detail: string, index = 0) {
    if (current) current = { ...current, detail, index, progress: null };
  },

  report(progress: Progress) {
    if (current) current = { ...current, progress };
  },

  end() {
    current = null;
  },
};
