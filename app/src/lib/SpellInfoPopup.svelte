<script lang="ts">
  // The `?` popup: what a spell does, how it stacks, and WHERE TO GET IT (wiki
  // acquisition rows). Never invents legacy-EQ acquisition data — an empty section
  // says so explicitly (spec §6).
  import type { AppState } from "./state.svelte";
  import { openUrl, spellInfo, type SpellInfo } from "./api";
  import { spellTierMana, spellTierTime, spellTierValue } from "./format";

  let { s, spellId, onclose }: {
    s: AppState;
    spellId: number;
    onclose: () => void;
  } = $props();

  let info = $state<SpellInfo | null>(null);
  let err = $state<string | null>(null);
  $effect(() => {
    info = null;
    err = null;
    const id = spellId;
    spellInfo(id)
      .then((i) => { if (spellId === id) info = i; })
      .catch((e) => { if (spellId === id) err = String(e); });
  });

  const tier = $derived(s.build.spell_tiers?.[spellId] ?? 0);
  const manaPct = $derived(s.staticData?.spell_tier_mana_pct ?? 6);
  const manaFloor = $derived(s.staticData?.spell_tier_mana_floor ?? 20);
  const castPct = $derived(s.staticData?.spell_tier_cast_pct ?? 4);

  function manaText(i: SpellInfo): string | null {
    if (i.mana == null) return null;
    if (tier === 0 || i.mana < manaFloor) return String(i.mana);
    return `≈${spellTierMana(i.mana, tier, manaPct)} (base ${i.mana}, provisional)`;
  }
  function timeText(secs: number | null): string | null {
    if (secs == null) return null;
    if (tier === 0) return `${secs}s`;
    return `${spellTierTime(secs, tier, castPct).toFixed(2)}s (base ${secs}s)`;
  }
  const SOURCE_LABEL: Record<string, string> = {
    VENDOR: "Vendor", DROP: "Loot", QUEST: "Quest", RESEARCH: "Research",
    CLASS_VENDOR: "Guild vendors", NOTE: "Note",
  };
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
  onkeydown={(e) => e.key === "Escape" && onclose()}
>
  <div class="win" role="dialog" aria-label="Spell information" tabindex="-1"
       onkeydown={(e) => e.key === "Escape" && onclose()}>
    <div class="titlebar">
      <span>{info?.name ?? "Spell"}</span>
      <button class="x" onclick={onclose} aria-label="close">✕</button>
    </div>
    <div class="body">
      {#if err}
        <p class="err">{err}</p>
      {:else if !info}
        <p class="dim">loading…</p>
      {:else}
        {#if info.description}<p class="desc">{info.description}</p>{/if}

        <div class="grid">
          <div>Classes:</div>
          <div>
            {#each info.class_levels as [c, lv, auto], i (c)}
              {i > 0 ? " · " : ""}{c} {lv}{auto ? " (autogranted)" : ""}
            {:else}—{/each}
          </div>
          {#if tier > 0}<div>Selected tier:</div><div class="gold">+{tier}</div>{/if}
          {#if manaText(info)}<div>Mana:</div><div>{manaText(info)}</div>{/if}
          {#if timeText(info.casting_time)}<div>Cast:</div><div>{timeText(info.casting_time)}</div>{/if}
          {#if timeText(info.recast_time)}<div>Recast:</div><div>{timeText(info.recast_time)}</div>{/if}
          {#if info.duration}<div>Duration:</div><div>{info.duration}</div>{/if}
          {#if info.target_type}<div>Target:</div><div>{info.target_type}</div>{/if}
          {#if info.resist_type}<div>Resist:</div><div>{info.resist_type}</div>{/if}
          {#if info.era}<div>Era:</div><div>{info.era}</div>{/if}
        </div>

        {#if info.buff_lines.length > 0}
          <div class="sect">
            <span class="secthead">Stacking line{info.buff_lines.length === 1 ? "" : "s"}:</span>
            {info.buff_lines.join(" · ")}
            <span class="dim"> — one buff per line; stronger members overwrite weaker ones</span>
          </div>
        {/if}

        <div class="sect">
          <span class="secthead">Where to obtain</span>
          {#if info.class_levels.some(([, , auto]) => auto)}
            <div class="acqrow auto">Autogranted at the listed level — no purchase needed.</div>
          {/if}
          {#if info.sources.length > 0}
            <table class="acq">
              <tbody>
                {#each info.sources as src, i (i)}
                  <tr>
                    <td class="stype">
                      {SOURCE_LABEL[src.source_type] ?? src.source_type}
                      {#if src.class_source}<span class="csrc">{src.class_source}</span>{/if}
                    </td>
                    {#if src.source_type === "NOTE" || (src.source_type === "RESEARCH" && src.raw_text)}
                      <td colspan="3" class="freetext">{src.raw_text ?? src.area ?? ""}</td>
                    {:else}
                      <td>{src.zone ?? "—"}</td>
                      <td>{src.npc ?? ""}</td>
                      <td class="dim">{src.area ?? ""} {src.loc ?? ""}</td>
                    {/if}
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
          {#if info.item_sources.length > 0}
            <div class="acqrow">Scroll found on: {info.item_sources.join(", ")}</div>
          {/if}
          {#if info.sources.length === 0 && info.item_sources.length === 0 && !info.class_levels.some(([, , auto]) => auto)}
            <div class="acqrow unconfirmed">
              Acquisition information has not yet been confirmed for EverQuest Legends.
            </div>
          {/if}
        </div>

        <button class="wikilink" onclick={() => openUrl(info!.wiki_url)}>
          Open on eqlwiki.com ↗
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; background: rgba(0, 0, 0, .55); z-index: 70;
    display: flex; align-items: flex-start; justify-content: center; padding-top: 10vh;
  }
  .win {
    width: 480px; max-width: 94vw; max-height: 76vh; overflow-y: auto;
    background: radial-gradient(ellipse at 30% 10%, #23262e 0%, #14161c 55%, #0d0f13 100%);
    border: 2px solid #8a7440; border-radius: 3px; color: #cbd;
    box-shadow: inset 0 0 0 1px #3a3f4a, 0 8px 40px rgba(0, 0, 0, .7);
  }
  .titlebar {
    display: flex; justify-content: space-between; align-items: center;
    background: linear-gradient(#1d2027, #14161c); border-bottom: 1px solid #3a3f4a;
    padding: 4px 10px; color: #d9c67a; font-size: .85rem;
  }
  .x { background: none; border: 1px solid #555; color: #aab; border-radius: 3px;
       cursor: pointer; font-size: .7rem; padding: 0 4px; }
  .x:hover { color: #fff; border-color: #c9b26a; }
  .body { padding: 10px 14px 14px; font-size: .8rem; }
  .desc { color: #cbd; line-height: 1.5; margin: 0 0 .6rem; }
  .grid { display: grid; grid-template-columns: auto 1fr; gap: 1px 12px; }
  .grid > div:nth-child(odd) { color: #9ab; }
  .gold { color: #c9b26a; font-weight: 600; }
  .sect { border-top: 1px solid #262b33; margin-top: .6rem; padding-top: .5rem; }
  .secthead { color: #c9b26a; font-variant: small-caps; letter-spacing: .05em; margin-right: .4rem; }
  .acq { border-collapse: collapse; width: 100%; margin-top: .3rem; font-size: .76rem; }
  .acq td { padding: 1px 8px 1px 0; border-bottom: 1px solid #20242b; }
  .stype { color: #6c9; white-space: nowrap; }
  .csrc { color: #c9b26a; font-size: .68rem; margin-left: 3px; }
  .freetext { color: #cbd; }
  .acqrow { margin-top: .3rem; color: #cbd; }
  .acqrow.auto { color: #6c9; }
  .acqrow.unconfirmed { color: #da5; font-style: italic; }
  .dim { color: #667; }
  .err { color: #f66; }
  .wikilink {
    margin-top: .7rem; background: #22262d; color: #6ac; border: 1px solid #345;
    border-radius: 6px; padding: 4px 10px; cursor: pointer; font-size: .78rem;
  }
  .wikilink:hover { color: #9cf; border-color: #6ac; }
</style>
