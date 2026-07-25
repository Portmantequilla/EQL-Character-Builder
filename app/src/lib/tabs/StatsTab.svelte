<script lang="ts">
  import type { AppState } from "../state.svelte";
  import EffectExplain from "../EffectExplain.svelte";
  import { orderStatKeys } from "../format";

  let { s }: { s: AppState } = $props();

  const r = $derived(s.result);
  const keys = $derived(r ? orderStatKeys(Object.keys(r.stats)) : []);
  const anyUnconfirmed = $derived(
    r ? keys.some((k) => r.stats[k].confidence !== "WIKI_CONFIRMED") : false
  );

  // ---- include/exclude buffs (display-side: base + gear always count; the toggle
  // recomputes raw/capped/over-cap without the spell/song buff column) ----
  let includeBuffs = $state(true);
  const ATTR_SET = new Set(["STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA"]);
  const RESIST_SET = new Set(["SV MAGIC", "SV FIRE", "SV COLD", "SV POISON", "SV DISEASE"]);
  const statCap = $derived(s.staticData?.stat_cap ?? 510);
  const resistCap = $derived(s.staticData?.resist_cap ?? 1000);
  /** displayed row math honoring the buffs toggle; attributes cap at stat_cap, resists
   *  (saves) at resist_cap — both EQL Discord community-reported (510 / 1000). */
  function rowFor(k: string) {
    const l = r!.stats[k];
    const equip = l.equipment + l.tier_bonus + l.item_effects;
    const buffs = includeBuffs ? l.buffs : 0;
    const raw = l.base + equip + buffs;
    const cap = ATTR_SET.has(k) ? statCap : RESIST_SET.has(k) ? resistCap : null;
    const capped = cap != null ? Math.min(raw, cap) : raw;
    return { l, equip, buffs, raw, cap, capped, over: Math.max(0, raw - capped) };
  }
  const fmt = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(1));

  // ---- quick stat explanations (hover) ----
  const STAT_INFO: Record<string, string> = {
    STR: "Strength — melee damage and how much weight you can carry.",
    STA: "Stamina — raises your hit points.",
    AGI: "Agility — helps you avoid being hit; very low AGI reduces AC.",
    DEX: "Dexterity — weapon proc rate and archery/thrown accuracy; helps bard instruments.",
    WIS: "Wisdom — mana pool for priests and hybrid priests (CLR, DRU, SHM, PAL, RNG, BST).",
    INT: "Intelligence — mana pool for int casters and hybrids (ENC, MAG, NEC, WIZ, SHD, BRD, MNK skills too).",
    CHA: "Charisma — better vendor prices; helps charm, mesmerize, and lull spells land.",
    AC: "Armor Class — reduces how hard (and how often) melee attacks hit you.",
    ATK: "Attack — raises your chance to land melee hits and hit for more. NOTE: this shows gear + buffs only; the large base from STR / offense / weapon skill isn't modeled yet (needs in-game data), so it reads well below the in-game number.",
    HP: "Hit Points — how much damage you can take before dying.",
    MANA: "Mana — the casting resource for your spell-using classes.",
    "HP REGEN": "Hit point regeneration per tick (6 seconds), on top of natural regen.",
    "MANA REGEN": "Mana regeneration per tick (6 seconds), on top of natural regen.",
    "SV MAGIC": "Magic resist — chance to resist or lessen hostile magic spells.",
    "SV FIRE": "Fire resist — chance to resist or lessen fire spells.",
    "SV COLD": "Cold resist — chance to resist or lessen cold spells.",
    "SV POISON": "Poison resist — chance to resist or lessen poison spells and effects.",
    "SV DISEASE": "Disease resist — chance to resist or lessen disease spells and effects.",
  };
  const statInfo = (k: string) => STAT_INFO[k] ?? `${k} — no description recorded yet.`;

  // ---- gear/augment effects (display-only until effect formulas are collected) ----
  const effects = $derived(r?.effect_overview ?? []);
  // stable identity per row: the list rebuilds on every gear change, so an index would
  // attach the open explanation to whatever row lands in that position next
  function fxKey(e: (typeof effects)[number]): string {
    return `${e.source_slot}|${e.kind}|${e.effect_name}|${e.via_augment ?? ""}`;
  }
  let openFx = $state<string | null>(null); // fxKey of the row whose explanation is open
</script>

{#if !r}
  <p class="muted">Waiting for calculation…</p>
{:else}
  <div class="topbar">
    <p class="haste">
      Haste: <strong>{r.equipment_haste_pct}%</strong> equipment ·
      <strong>{r.buff_haste_pct}%</strong> buffs
    </p>
    <button
      class="bufftoggle"
      class:on={includeBuffs}
      onclick={() => (includeBuffs = !includeBuffs)}
      title="include or exclude buffs from spells, songs, and gear-sourced casts in the totals"
    >
      {includeBuffs ? "✓ Buffs included" : "Buffs excluded"}
    </button>
  </div>

  <table>
    <thead>
      <tr>
        <th>Stat</th><th class="num">Base</th><th class="num">Equipment</th>
        <th class="num" title="flat stats from worn item effects + worn Exaltation augments">Item FX</th>
        <th class="num">Buffs</th><th class="num">Raw</th><th class="num">Cap</th>
        <th class="num">Capped</th><th class="num">Over-cap</th>
      </tr>
    </thead>
    <tbody>
      {#each keys as k (k)}
        {@const row = rowFor(k)}
        <tr>
          <td class="key">
            <span class="statname" title={statInfo(k)}>{k}</span>
            {#if row.l.confidence !== "WIKI_CONFIRMED"}
              <span class="conf" title={row.l.confidence}>†</span>
            {/if}
          </td>
          <td class="num">{row.l.base}</td>
          <td
            class="num"
            class:tierb={row.l.tier_bonus !== 0}
            title={row.l.tier_bonus !== 0
              ? `${row.l.equipment} base + ${row.l.tier_bonus} from item upgrade tiers`
              : undefined}
          >{row.l.equipment + row.l.tier_bonus}</td>
          <td class="num" class:fxb={row.l.item_effects !== 0}>{fmt(row.l.item_effects)}</td>
          <td class="num" class:excluded={!includeBuffs && row.l.buffs !== 0}>
            {includeBuffs ? fmt(row.l.buffs) : "—"}
          </td>
          <td class="num">{fmt(row.raw)}</td>
          <td class="num capn">{row.cap ?? "—"}</td>
          <td class="num total">{fmt(row.capped)}</td>
          <td class="num" class:over={row.over > 0}>{fmt(row.over)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  <p class="capnote2">
    buffed caps (EQL Discord): attributes <strong>{statCap}</strong> (<code>stat_cap</code>),
    saves/resists <strong>{resistCap}</strong> (<code>resist_cap</code>) — both editable in
    Settings; anything above lands in Over-cap. Hover a stat name for what it does.
  </p>

  {#if anyUnconfirmed}
    <p class="footnote">
      † value not wiki-confirmed — hover the icon for the confidence level
      (unverified numbers need an in-game check, plan §8.2).
    </p>
  {/if}

  <h3 class="fxhead">Item &amp; augment effects ({effects.length})</h3>
  {#if effects.length === 0}
    <p class="muted">No worn item or socketed augment grants an effect.</p>
  {:else}
    <p class="fxnote">
      <strong class="folded">Worn effects with flat stats are folded into the Item FX
      column</strong> above (an effect also running as an active buff counts once, as
      the buff). Percent effects count where they belong (worn haste in the haste line);
      click and proc effects apply via the Buffs tab's "Add Other Buff". Focus effects
      apply on the Spell Manager page. Click a row for what the effect does.
    </p>
    <ul class="fxlist">
      <!-- index folded into the each key: two identical (slot,kind,name,aug) rows must
           not crash the keyed each (Svelte 5 hard-throws on duplicates — house rule) -->
      {#each effects as e, i (fxKey(e) + "#" + i)}
        {@const k = fxKey(e)}
        <li class:gated={e.level_gated}>
          <button
            class="fxrow"
            aria-expanded={openFx === k}
            onclick={() => (openFx = openFx === k ? null : k)}
          >
            <span class="fxlabel">{e.label}:</span>
            <span class="fxname" class:viaaug={e.via_augment != null}>{e.effect_name}</span>
            {#if e.required_level != null}<span class="freq">(Req Level {e.required_level})</span>{/if}
            {#if e.level_gated}<span class="gated-badge">above your level</span>{/if}
            <span class="fxsrc">
              {e.source_slot} · {e.source_item}{e.via_augment ? ` · via ${e.via_augment}` : ""}
            </span>
          </button>
          {#if e.warnings.length > 0}
            {#each e.warnings as w, wi (wi)}<div class="fxwarn">⚠ {w}</div>{/each}
          {/if}
          {#if openFx === k}
            <div class="fxdetail">
              <EffectExplain spellId={e.spell_id} effectName={e.effect_name} />
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<style>
  .muted { color: #667; }
  .topbar { display: flex; gap: .8rem; align-items: center; flex-wrap: wrap; }
  .haste { background: #1c1f26; display: inline-block; padding: .35rem .8rem; border-radius: 8px; }
  .haste strong { color: #fc6; }
  .bufftoggle {
    background: #22262d; color: #778; border: 1px solid #333; border-radius: 6px;
    padding: 5px 12px; cursor: pointer; font: inherit; font-size: .8rem;
  }
  .bufftoggle:hover { color: #cbd; }
  .bufftoggle.on { color: #6af; border-color: #46c; background: #141a24; }
  .statname { cursor: help; border-bottom: 1px dotted #4a5568; }
  .fxb { color: #d4f; font-weight: 600; }
  .excluded { color: #556; text-decoration: line-through; }
  .capn { color: #89a; }
  .capnote2 { color: #987; font-size: .74rem; max-width: 640px; }
  .capnote2 code { background: #12151c; border: 1px solid #2a2f38; border-radius: 4px; padding: 0 4px; }
  .folded { color: #6c9; }
  table { border-collapse: collapse; font-size: .85rem; min-width: 560px; }
  th { text-align: left; color: #89a; font-weight: 600; padding: 3px 14px 3px 0; border-bottom: 1px solid #333; }
  th.num, td.num { text-align: right; }
  td { padding: 2px 14px 2px 0; border-bottom: 1px solid #20242b; }
  .key { color: #9ab; }
  .total { color: #fc6; font-weight: 600; }
  .over { color: #f90; font-weight: 600; }
  .tierb { color: #c9b26a; font-weight: 600; }
  .conf { color: #fa6; cursor: help; margin-left: 2px; }
  .footnote { color: #987; font-size: .75rem; max-width: 560px; }

  /* ---- item & augment effects (display-only list) ---- */
  .fxhead { color: #c9b26a; font-variant: small-caps; letter-spacing: .08em; margin: 1.2rem 0 .3rem; }
  .fxnote { color: #987; font-size: .75rem; max-width: 620px; margin: 0 0 .5rem; }
  .fxnote strong { color: #da5; }
  .fxlist { list-style: none; margin: 0; padding: 0; max-width: 720px; }
  .fxlist li { border-bottom: 1px solid #20242b; }
  .fxlist li.gated { opacity: .65; }
  .fxrow {
    display: flex; gap: .45rem; align-items: baseline; width: 100%; text-align: left;
    background: none; border: none; color: #cbd; padding: 4px 2px; cursor: pointer;
    font: inherit; font-size: .8rem; flex-wrap: wrap;
  }
  .fxrow:hover { background: #171a20; }
  .fxlabel { color: #9ab; }
  .fxname { color: #6ac; }
  .fxname.viaaug { color: #d4f; }
  .freq { color: #99a; font-size: .72rem; }
  .gated-badge {
    color: #da5; font-size: .66rem; border: 1px solid #543; border-radius: 4px; padding: 0 4px;
  }
  .fxsrc { margin-left: auto; color: #667; font-size: .7rem; }
  .fxwarn { color: #da5; font-size: .7rem; margin: 0 0 3px 12px; }
  .fxdetail {
    margin: 2px 0 6px 12px; padding: 6px 9px;
    background: #12151c; border: 1px solid #2a2f38; border-radius: 6px;
    max-width: 560px;
  }
</style>
