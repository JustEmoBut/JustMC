<script lang="ts">
  import Icon from "./Icon.svelte";

  /**
   * A player's face, cut from their own skin: the 8×8 face at (8, 8) with the
   * hat layer at (40, 8) drawn over it, scaled up without smoothing. Drawn on
   * a canvas, which a cross-origin image may be painted onto freely; only
   * reading its pixels back would be refused. No skin (an offline account,
   * or one not read since signing in) shows the generic icon.
   */
  let { url, size = 24 }: { url: string; size?: number } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let failed = $state(false);

  $effect(() => {
    const target = canvas;
    if (!url || !target) return;
    failed = false;
    const image = new Image();
    image.onload = () => {
      const g = target.getContext("2d");
      if (!g) return;
      g.imageSmoothingEnabled = false;
      g.clearRect(0, 0, size, size);
      g.drawImage(image, 8, 8, 8, 8, 0, 0, size, size);
      // A legacy 64×32 skin has a hat layer too, at the same place.
      g.drawImage(image, 40, 8, 8, 8, 0, 0, size, size);
    };
    image.onerror = () => (failed = true);
    image.src = url;
  });
</script>

{#if url && !failed}
  <canvas bind:this={canvas} width={size} height={size} aria-hidden="true"></canvas>
{:else}
  <span class="none" style:width="{size}px" style:height="{size}px">
    <Icon name="user" size={Math.round(size * 0.6)} />
  </span>
{/if}

<style>
  canvas,
  .none {
    flex: none;
    border-radius: 4px;
    background: var(--border);
  }

  .none {
    display: grid;
    place-items: center;
    color: var(--text-faint);
  }
</style>
