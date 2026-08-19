/** Transient status messages. One shared list; components just call `notify`. */
export interface Toast {
  id: number;
  kind: "info" | "error";
  text: string;
}

let nextId = 0;

export const toasts = $state<{ items: Toast[] }>({ items: [] });

export function notify(text: string, kind: Toast["kind"] = "info") {
  const id = nextId++;
  toasts.items.push({ id, kind, text });
  // Errors stay long enough to read a stack-ish message; info is a quick note.
  setTimeout(() => dismiss(id), kind === "error" ? 9000 : 3500);
}

export function dismiss(id: number) {
  const i = toasts.items.findIndex((t) => t.id === id);
  if (i !== -1) toasts.items.splice(i, 1);
}
