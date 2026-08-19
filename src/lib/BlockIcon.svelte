<script lang="ts">
  /**
   * A stand-in instance icon: a block drawn in three-quarter view, coloured
   * from the instance name.
   *
   * Instances have no user-chosen icon yet, and a grid of identical grey
   * squares is unscannable. The hue is derived from the name so it is stable —
   * the icon never changes under the user — and distinct enough that people
   * recognise their packs by colour.
   */
  let { name, size = 40, fabric = false }: {
    name: string;
    size?: number;
    fabric?: boolean;
  } = $props();

  function hue(text: string) {
    let hash = 0;
    for (let i = 0; i < text.length; i++) {
      hash = (hash * 31 + text.charCodeAt(i)) % 360;
    }
    return hash;
  }

  const h = $derived(hue(name));
</script>

<span
  class="block"
  style:width="{size}px"
  style:height="{size}px"
  style:--top="hsl({h} 44% 62%)"
  style:--face="hsl({h} 42% 47%)"
  style:--side="hsl({h} 40% 33%)"
>
  {#if fabric}
    <!-- Modded instances are worth telling apart at a glance; a legible letter
         beats the texture swatch this used to be. -->
    <span class="loader" style:--chip="{Math.max(11, size * 0.4)}px" title="Fabric">F</span>
  {/if}
</span>

<style>
  /* A block, drawn the way the game draws one: a lit top face, a front face,
     a shaded right face, and a coarse pixel grid over all three. The grid is
     what makes it read as a block rather than a rounded gradient chip -- it is
     the whole reason this icon is recognisable at 32px in a wall of others.

     Layer order matters: the pixel grid and the side face paint over the
     top/front gradient, which is opaque and would otherwise hide them. */
  .block {
    position: relative;
    display: block;
    flex: none;
    border-radius: 2px;
    background:
      repeating-linear-gradient(90deg, rgb(0 0 0 / 0.14) 0 1px, transparent 1px 25%),
      repeating-linear-gradient(rgb(255 255 255 / 0.06) 0 1px, transparent 1px 25%),
      linear-gradient(115deg, rgb(255 255 255 / 0.09) 0 40%, transparent 40%),
      linear-gradient(to right, transparent 0 74%, var(--side) 74% 100%),
      linear-gradient(to bottom, var(--top) 0 24%, var(--face) 24% 100%);
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 0.45),
      inset 0 1px 0 rgb(255 255 255 / 0.18),
      0 2px 5px rgb(0 0 0 / 0.4);
  }

  .loader {
    position: absolute;
    right: -3px;
    bottom: -3px;
    width: var(--chip);
    height: var(--chip);
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    color: var(--text-dim);
    font-size: calc(var(--chip) * 0.62);
    font-weight: 700;
    line-height: 1;
  }
</style>
