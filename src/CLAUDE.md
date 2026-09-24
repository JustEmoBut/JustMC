# Frontend conventions

## A snippet must be a direct child of the component it is passed to

`{#snippet footer()}` reaches `Modal` as a prop only when it sits directly
inside `<Modal>…</Modal>`. Nested one level deeper — inside an `{#if}` that
switches the dialog's mode, say — Svelte treats it as a local snippet, passes
nothing, and the component renders no footer. Nothing warns: not
`svelte-check`, not the build. The Add Instance dialog lost its Cancel and
Create buttons that way, and the dialog still looked otherwise normal.

When a footer's content depends on state, put the condition *inside* the
snippet, never around it.

## A dialog closes on Escape and the close button, never the backdrop

`Modal` deliberately has no backdrop click handler. Most dialogs here hold a
half-filled form (a new instance, instance settings, an export), and a stray
click beside one is not a decision to discard it.

## Scroll containers

Setting `overflow-y: auto` makes the browser compute `overflow-x` as `auto`
too, so a few pixels of overhang draw a scrollbar across a whole dialog. Every
scrolling container in the app pairs it with `overflow-x: hidden`; content that
genuinely cannot wrap (code blocks, tables) scrolls inside its own box, and a
table needs `display: block` before it will.

## Browsing is paged, filtered and racy

`Mods.svelte` pages Modrinth by **how many results it already holds**, never by
a page counter — appending then cannot drift out of step with the list. Each
search carries a run id so a slow earlier request cannot append to a newer
one's results, and "hide installed" tops the list up rather than leaving two
rows behind, because the installed set arrives after the first page and can
filter almost all of it away. Toggling the filter costs no request: the hits
are already held, only the derived view changes.

## One window for one long job

Downloads report through a single `install-progress` event, so the label has to
come from whoever started them: `task.begin`/`step`/`end` in
`src/lib/task.svelte.ts`, with `App.svelte` subscribing to the event once and
`TaskWindow.svelte` rendering whatever is running. The status strip describes
the selection, never the download.

## Instance actions live in one place

`src/lib/actions.ts` holds open-folder, export, duplicate, stop and delete.
The side panel, the settings dialog and the right-click menu (`ContextMenu.svelte`) all call it;
adding an action there is what makes it appear everywhere it belongs.

