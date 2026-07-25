<script lang="ts">
  // The augment's own description card, modeled on the game's Exaltation item window
  // (Gnoll Hide Lariat reference): red name, Augmentation flags, class/race rules,
  // which slot types it fits, the effect it grants, and what that effect does.
  import type { AugmentInfo } from "./api";
  import EffectExplain from "./EffectExplain.svelte";
  import { hideOnError, iconUrl } from "./format";

  let { aug }: { aug: AugmentInfo } = $props();

  const SOCKET_LABEL: Record<string, string> = {
    FOCUS: "Focus Exaltation", CLICK: "Click Exaltation",
    WORN: "Worn Exaltation", PROC: "Proc Exaltation",
  };
  const EFFECT_LABEL: Record<string, string> = {
    PROC: "Combat Effect", WORN: "Worn Effect",
    CLICK: "Click Effect", FOCUS: "Focus Effect",
  };
  // the game lists "Augmentation" among the flags; source flags follow when known
  const flagline = $derived(
    aug.flags ? `Augmentation, ${aug.flags}` : "Augmentation"
  );
  const handSlots = $derived(
    aug.slots.filter((sl) => ["PRIMARY", "SECONDARY", "RANGE"].includes(sl))
  );
</script>

<div class="augcard">
  <div class="head">
    <span class="ic">
      {#if aug.icon_id != null}
        {#key aug.icon_id}
          <img src={iconUrl(aug.icon_id)} alt="" draggable="false" onerror={hideOnError} />
        {/key}
      {/if}
    </span>
    <div>
      <div class="aname">{aug.name}</div>
      <div class="row">{flagline}</div>
      {#if aug.classes.length > 0}<div class="row">Class: {aug.classes.join(" ")}</div>{/if}
      {#if aug.races.length > 0}<div class="row">Race: {aug.races.join(" ")}</div>{/if}
      {#if handSlots.length > 0}<div class="row">{handSlots.join(" ")}</div>{/if}
    </div>
  </div>

  <div class="fits">This Augmentation fits in slot types: ({SOCKET_LABEL[aug.socket] ?? aug.socket})</div>

  <div class="fx">
    {EFFECT_LABEL[aug.socket] ?? "Effect"}:
    <span class="fxname">{aug.effect_name}</span>
    {#if aug.required_level != null}<span class="freq">(Req Level {aug.required_level})</span>{/if}
  </div>

  <EffectExplain spellId={aug.spell_id} effectName={aug.effect_name} />

  <div class="extract">extracted from a +4 source item{aug.era ? ` · ${aug.era}` : ""}</div>
</div>

<style>
  .augcard {
    width: 300px; padding: 8px 10px; font-size: .74rem; line-height: 1.45;
    background: radial-gradient(ellipse at 30% 10%, #23262e 0%, #14161c 55%, #0d0f13 100%);
    border: 1px solid #8a7440; border-radius: 3px; color: #cbd;
    box-shadow: inset 0 0 0 1px #3a3f4a, 0 6px 20px rgba(0, 0, 0, .7);
  }
  .head { display: flex; gap: 8px; margin-bottom: 4px; }
  .ic {
    width: 32px; height: 32px; flex-shrink: 0;
    background: #101318; border: 1px solid #2e3440; border-radius: 2px;
    display: flex; align-items: center; justify-content: center;
  }
  .ic img { width: 28px; height: 28px; image-rendering: pixelated; }
  .aname { color: #e34; font-weight: 700; }
  .row { color: #cbd; }
  .fits { color: #cbd; margin: 4px 0; border-top: 1px solid #262b33; padding-top: 4px; }
  .fx { margin-bottom: 3px; }
  .fxname { color: #d4f; }
  .freq { color: #99a; margin-left: 3px; }
  .extract { color: #667; font-size: .66rem; font-style: italic; margin-top: 4px; }
</style>
