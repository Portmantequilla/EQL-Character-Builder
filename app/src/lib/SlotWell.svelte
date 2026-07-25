<script lang="ts">
  import type { Snippet } from "svelte";
  import { hideOnError, iconUrl } from "./format";

  let { iconId, iconSrc = null, label, filled = false, glow = "none", selected = false, tier = 0, cornerLabel = null, locked = false, onclick, onclear, tooltip }: {
    iconId: number | null;
    /** ready-made icon URL (spell gems); takes precedence over iconId when set */
    iconSrc?: string | null;
    label: string;                 // slot name shown inside an EMPTY well
    filled?: boolean;
    glow?: "none" | "red" | "yellow" | "green" | "orange";
    selected?: boolean;
    tier?: number;                 // item/spell upgrade tier: gold "+N" corner badge when > 0
    /** TOP-LEFT corner badge (e.g. the spell's required cast level "L34"); display only */
    cornerLabel?: string | null;
    /** unavailable well: dim + lock glyph, never clickable (tooltip still shows why) */
    locked?: boolean;
    onclick?: () => void;
    onclear?: () => void;          // renders the small x on filled wells
    tooltip?: Snippet;
  } = $props();

  let hover = $state(false);
</script>

<div class="wrap">
  <button
    type="button"
    class="well glow-{glow}"
    class:selected
    class:locked
    aria-disabled={locked}
    onclick={() => { if (!locked) onclick?.(); }}
    onmouseenter={() => (hover = true)}
    onmouseleave={() => (hover = false)}
    onfocus={() => (hover = true)}
    onblur={() => (hover = false)}
  >
    {#if locked}
      <span class="lock" aria-hidden="true">&#128274;</span>
    {:else if filled && iconSrc != null}
      {#key iconSrc}
        <img src={iconSrc} alt="" draggable="false" onerror={hideOnError} />
      {/key}
    {:else if filled && iconId != null}
      <!-- keyed so a well re-tries the img when the icon changes after an onerror hide -->
      {#key iconId}
        <img src={iconUrl(iconId)} alt="" draggable="false" onerror={hideOnError} />
      {/key}
    {:else if !filled}
      <span class="lbl">{label}</span>
    {/if}
    <!-- filled + no icon (or img error): bare empty-looking well; the tooltip carries the info -->
  </button>

  {#if filled && !locked && tier > 0}
    <span class="tierbadge">+{tier}</span>
  {/if}

  {#if filled && !locked && cornerLabel}
    <span class="cornerbadge">{cornerLabel}</span>
  {/if}

  {#if filled && !locked && onclear}
    <button type="button" class="clearx" title="clear slot" onclick={onclear}>×</button>
  {/if}

  {#if hover && tooltip}
    <div class="tip">{@render tooltip()}</div>
  {/if}
</div>

<style>
  .wrap { position: relative; width: 52px; height: 52px; }
  .well {
    width: 52px; height: 52px; padding: 0; margin: 0;
    background: #101318;
    border: 2px groove #2e3440;
    border-radius: 2px;
    box-shadow: inset 0 0 6px #000;
    display: flex; align-items: center; justify-content: center;
    cursor: pointer; font: inherit;
  }
  .well:hover { border-color: #4a5568; }
  .well.selected { border-color: #c9b26a; border-style: solid; }
  /* locked: dim + not clickable, but still hoverable so the tooltip can say why */
  .well.locked {
    cursor: default; opacity: .42;
    background: #0b0d11; border-style: dashed; border-color: #262c36;
  }
  .well.locked:hover { border-color: #3a4250; }
  .lock { font-size: .8rem; color: #5a6577; line-height: 1; }
  .well img { width: 40px; height: 40px; image-rendering: pixelated; pointer-events: none; }
  .lbl {
    font-size: .55rem; color: #4a5568; text-transform: uppercase;
    letter-spacing: .03em; line-height: 1.15; text-align: center; padding: 0 2px;
    overflow: hidden; word-break: break-word;
  }
  .glow-red    { box-shadow: inset 0 0 6px #000, 0 0 8px #a33; }
  .glow-yellow { box-shadow: inset 0 0 6px #000, 0 0 8px #aa3; }
  .glow-green  { box-shadow: inset 0 0 6px #000, 0 0 8px #2a6; }
  .glow-orange { box-shadow: inset 0 0 6px #000, 0 0 8px #c73; }
  .tierbadge {
    position: absolute; top: 2px; right: 2px; z-index: 4;
    background: rgba(13, 15, 20, .88); color: #c9b26a;
    border: 1px solid #8a7440; border-radius: 2px;
    font-size: .58rem; line-height: 1; padding: 1px 2px;
    pointer-events: none; font-variant: small-caps;
  }
  /* top-left counterpart to the tier badge: the spell's required cast level */
  .cornerbadge {
    position: absolute; top: 2px; left: 2px; z-index: 4;
    background: rgba(13, 15, 20, .88); color: #9ab;
    border: 1px solid #3a4250; border-radius: 2px;
    font-size: .58rem; line-height: 1; padding: 1px 2px;
    pointer-events: none;
  }
  .clearx {
    position: absolute; top: -6px; right: -6px; width: 16px; height: 16px;
    padding: 0; line-height: 1; font-size: .7rem;
    background: #2a2024; color: #c66; border: 1px solid #a33; border-radius: 50%;
    cursor: pointer; z-index: 5;
  }
  .clearx:hover { color: #f88; background: #3a2428; }
  .tip {
    position: absolute; top: calc(100% + 6px); left: 50%; transform: translateX(-50%);
    min-width: 230px; max-width: 350px; z-index: 60;
    background: #0d0f14; border: 1px solid #8a7440; border-radius: 3px;
    padding: 6px 9px; font-size: .74rem; color: #bcd; text-align: left;
    box-shadow: 0 4px 14px rgba(0, 0, 0, .7);
    pointer-events: none;
  }
</style>
