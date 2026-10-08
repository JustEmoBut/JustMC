<script lang="ts">
  import {
    api,
    errorMessage,
    type Instance,
    type Server,
    type ServerStatus,
  } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { notify } from "./toast.svelte";

  let {
    instance,
    running,
    onclose,
    embedded = false,
    onjoin,
  }: {
    instance: Instance;
    /** The game rewrites servers.dat on exit and would drop an edit made under it. */
    running: boolean;
    onclose: () => void;
    /** Shown as a page of a window rather than as its own dialog. */
    embedded?: boolean;
    /** Launch straight into this address; the dialog closes behind it. */
    onjoin: (address: string) => void;
  } = $props();

  /**
   * Whether this version understands Quick Play, from its own metadata. Null
   * until the answer lands, so nothing is offered and nothing is denied yet.
   */
  let quickPlay = $state<boolean | null>(null);
  const canJoin = $derived(quickPlay === true && !running);

  $effect(() => {
    // An unreachable manifest or an unknown version leaves the button showing:
    // the launch path checks again and gives the honest error there.
    api
      .quickPlaySupported(instance.id)
      .then((yes) => (quickPlay = yes))
      .catch(() => (quickPlay = true));
  });

  function join(server: Server) {
    if (!canJoin) return;
    // Launch first: closing clears the caller's reference to this instance, so
    // handing the address over afterwards hands it to nothing.
    onjoin(server.ip);
    onclose();
  }

  let servers = $state<Server[]>([]);
  let loading = $state(true);
  let adding = $state(false);
  let newName = $state("");
  let newIp = $state("");
  /** Index of the server whose delete button is armed; only ever one. */
  let confirming = $state(-1);

  /** Status per address, so two entries pointing at one server share a ping. */
  let status = $state<Record<string, ServerStatus | "pending" | "offline">>({});

  async function refresh() {
    loading = true;
    try {
      servers = await api.listServers(instance.id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    loading = false;
    pingAll();
  }

  /**
   * One request per server, all in flight at once: a dead address costs the
   * whole five second timeout, and waiting them out in turn would make a list
   * of ten take a minute.
   */
  function pingAll() {
    for (const server of servers) {
      if (!server.ip || status[server.ip]) continue;
      status[server.ip] = "pending";
      api
        .pingServer(server.ip)
        // Braces, not a concise body: an assignment to a reactive object
        // evaluates to the right-hand side rather than to what was stored, and
        // returning that is what Svelte warns about.
        .then((answer) => {
          status[server.ip] = answer;
        })
        .catch(() => {
          status[server.ip] = "offline";
        });
    }
  }

  function retry(ip: string) {
    delete status[ip];
    pingAll();
  }

  async function add(event: Event) {
    event.preventDefault();
    adding = true;
    try {
      await api.addServer(instance.id, newName, newIp);
      newName = "";
      newIp = "";
      await refresh();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
    adding = false;
  }

  async function remove(server: Server) {
    confirming = -1;
    try {
      await api.removeServer(instance.id, server.index, server.ip);
      await refresh();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  }

  $effect(() => {
    refresh();
  });

  /** The line under the name: what the server said, or why it said nothing. */
  function detail(server: Server) {
    const answer = status[server.ip];
    if (!answer || answer === "pending") return "Pinging…";
    if (answer === "offline") return "Offline";
    const parts = [`${answer.online}/${answer.max} players`, `${answer.latency_ms} ms`];
    if (answer.version) parts.push(answer.version);
    return parts.join(" · ");
  }

  /** The server's own icon when it answered with one, else the cached one. */
  function icon(server: Server) {
    const answer = status[server.ip];
    if (answer && answer !== "pending" && answer !== "offline" && answer.favicon) {
      return answer.favicon;
    }
    return server.icon;
  }

  function motd(server: Server) {
    const answer = status[server.ip];
    return answer && answer !== "pending" && answer !== "offline" ? answer.motd : "";
  }
</script>

<Modal title="Servers — {instance.name}" {onclose} {embedded} width="640px">
  {#if !running && quickPlay === false}
    <p class="muted note">
      Joining from here needs Minecraft 1.20 or newer; this instance is on
      {instance.mc_version}.
    </p>
  {/if}

  {#if running}
    <p class="warn">
      {instance.name} is running. It rewrites the server list when it quits, so changes made here
      would be lost.
    </p>
  {/if}

  {#if loading}
    <p class="muted">Reading the server list…</p>
  {:else if servers.length === 0}
    <p class="muted">This instance has no servers yet.</p>
  {:else}
    <ul>
      {#each servers as server (server.index)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <li class:joinable={canJoin} ondblclick={() => join(server)}>
          {#if icon(server)}
            <img src={icon(server)} alt="" width="32" height="32" />
          {:else}
            <div class="noicon"></div>
          {/if}

          <div class="what">
            <strong>{server.name || server.ip}</strong>
            <span class="muted">{server.ip} · {detail(server)}</span>
            {#if motd(server)}
              <span class="motd">{motd(server)}</span>
            {/if}
          </div>

          {#if confirming === server.index}
            <span class="muted">Remove it?</span>
            <button onclick={() => (confirming = -1)}>Cancel</button>
            <button class="really" onclick={() => remove(server)}>Remove</button>
          {:else}
            {#if canJoin}
              <button onclick={() => join(server)} title="Launch straight into this server">
                <Icon name="play" />
                Join
              </button>
            {/if}
            <button onclick={() => retry(server.ip)} title="Ping again">
              <Icon name="refresh" />
            </button>
            <button
              class="destructive"
              disabled={running}
              onclick={() => (confirming = server.index)}
              aria-label="Remove {server.name || server.ip}"
            >
              <Icon name="trash" />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <form onsubmit={add}>
    <input bind:value={newName} placeholder="Name (optional)" disabled={running || adding} />
    <input bind:value={newIp} placeholder="address:port" disabled={running || adding} />
    <button type="submit" disabled={running || adding || !newIp.trim()}>
      <Icon name="plus" />
      Add
    </button>
  </form>
</Modal>

<style>
  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--bg-inset);
  }

  /* The base button style is not flex, so an icon and a label would stack. */
  li button,
  form button {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  li img,
  .noicon {
    width: 32px;
    height: 32px;
    border-radius: 4px;
    flex: none;
    image-rendering: pixelated;
  }

  .noicon {
    background: var(--bg-raised);
  }

  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .what strong {
    font-size: 13px;
  }

  .what .muted,
  .motd {
    font-size: 11px;
  }

  .motd {
    color: var(--text-dim);
    /* A MOTD is two lines of the server's own advertising; it does not get to
       stretch the row. */
    max-height: 2.6em;
    overflow: hidden;
  }

  form {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  form input {
    flex: 1;
    min-width: 0;
  }

  .joinable {
    cursor: default;
  }

  .note {
    margin-bottom: 10px;
    font-size: 12px;
  }

  .warn {
    margin-bottom: 10px;
    font-size: 12px;
    color: #fbbf24;
  }

  .really,
  .destructive:hover:not(:disabled) {
    color: var(--danger);
  }
</style>
