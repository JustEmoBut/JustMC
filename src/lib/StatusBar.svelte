<script lang="ts">
  import { loaderName, type Instance } from "./api";

  let { selected, instances }: { selected: Instance | undefined; instances: Instance[] } = $props();

  /** Every instance that counts, so the number matches what the tiles say. */
  const totalPlayed = $derived(
    instances.reduce((sum, i) => sum + (i.count_play_time ? i.play_time : 0), 0)
  );

  /** Hours once there are any; a launcher total below a minute is not news. */
  function totalPlayTime(seconds: number) {
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes} min played`;
    return `${Math.floor(minutes / 60)} h played`;
  }
</script>

<!-- One line of facts about what is selected -- the place the eye already
     goes on a desktop tool. -->
<div class="status data">
  {#if selected}
    <span>Minecraft {selected.mc_version}</span>
    <span class="sep">·</span>
    <span>{loaderName(selected.loader)}</span>
    <span class="sep">·</span>
    <span>{selected.memory_mb} MB</span>
    {#if selected.java_path}
      <span class="sep">·</span>
      <span>custom Java</span>
    {/if}
  {/if}
  <span class="spacer"></span>
  {#if totalPlayed}
    <span>{totalPlayTime(totalPlayed)}</span>
    <span class="sep">·</span>
  {/if}
  <span>{instances.length} {instances.length === 1 ? "instance" : "instances"}</span>
</div>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px;
    height: 30px;
    border-top: 1px solid var(--border);
    color: var(--text-faint);
    flex: none;
  }

  .sep {
    opacity: 0.5;
  }
</style>
