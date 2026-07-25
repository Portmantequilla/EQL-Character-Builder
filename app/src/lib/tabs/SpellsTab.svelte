<script lang="ts">
  import type { AppState } from "../state.svelte";
  import { externalReceivable, querySpells, setFormula, type SpellRow } from "../api";
  import SpellInfoPopup from "../SpellInfoPopup.svelte";
  import { spellTierMana, spellTierTime, spellTierValue } from "../format";

  let { s }: { s: AppState } = $props();

  let infoSpell = $state<number | null>(null); // the `?` popup target

  // ---- class buttons (power-planner): ALL 16 classes; the build's three are the
  // default list, but any class can be toggled in to browse/borrow its spells ----
  let customized = $state(false); // user changed the selection -> stop tracking the build
  let selClasses = $state<string[]>([]);
  $effect(() => {
    const bc = [...s.build.classes];
    if (!customized) selClasses = bc;
  });
  const allClasses = $derived(s.staticData?.classes ?? []);
  function toggleClass(c: string) {
    customized = true;
    selClasses = selClasses.includes(c)
      ? selClasses.filter((x) => x !== c)
      : [...selClasses, c];
  }
  function resetClasses() {
    customized = false;
    selClasses = [...s.build.classes];
  }

  // ---- external buffs: spells CAST ON YOU by other players (any class) ----
  const externals = $derived(new Set(s.build.external_buffs ?? []));
  function toggleExternal(id: number) {
    const cur = s.build.external_buffs ?? [];
    s.build.external_buffs = cur.includes(id)
      ? cur.filter((x) => x !== id)
      : [...cur, id]; // fresh array -> resolve pipeline re-fires
  }
  const plan = $derived(s.result?.buff_plan ?? null);

  // which spells can actually be RECEIVED from another player (non-self-only buff-line
  // members): ~78% of beneficial spells can't — their Cast button must say so, not
  // silently count a buff the engine will (correctly) ignore
  let receivable = $state<Set<number> | null>(null); // null = still loading
  $effect(() => {
    externalReceivable()
      .then((ids) => (receivable = new Set(ids)))
      .catch((e) => (s.error = String(e)));
  });

  // ---- buff cap slider: writes the buff_slot_cap formula (engine-enforced) ----
  // capDraft holds the user's value until the RESOLVED plan reports it back —
  // clearing it earlier made the slider snap to the old cap during the round-trip
  let capDraft = $state<number | null>(null);
  const cap = $derived(capDraft ?? plan?.buff_slot_cap ?? 30);
  $effect(() => {
    if (capDraft != null && plan?.buff_slot_cap === capDraft) capDraft = null;
  });
  async function commitCap(n: number) {
    try {
      await setFormula("buff_slot_cap", String(n), false);
      s.resolveNonce++; // snapshot refreshed Rust-side — force a re-resolve
    } catch (e) {
      s.error = String(e);
      capDraft = null; // commit failed: show the truth again
    }
  }

  // spell tier scaling, formula-driven (Settings-editable; defaults mirror the
  // community-reconstructed rules: +6%/tier dmg/heal, -6% mana ≈, -4% cast/reuse)
  const dmgPct = $derived(s.staticData?.spell_tier_scaling_pct ?? 6);
  const manaPct = $derived(s.staticData?.spell_tier_mana_pct ?? 6);
  const manaFloor = $derived(s.staticData?.spell_tier_mana_floor ?? 20);
  const castPct = $derived(s.staticData?.spell_tier_cast_pct ?? 4);
  const reagentPct = $derived(s.staticData?.reagent_conserve_pct_per_tier ?? 10);

  /** tiered mana display: ≈N when scaled (provisional), base when under the floor. */
  function manaAt(base: number | null, tier: number): { text: string; scaled: boolean } {
    if (base == null) return { text: "—", scaled: false };
    if (tier === 0 || base < manaFloor) return { text: String(base), scaled: false };
    return { text: `≈${spellTierMana(base, tier, manaPct)}`, scaled: true };
  }
  function castAt(secs: number | null, tier: number): { text: string; scaled: boolean } {
    if (secs == null) return { text: "—", scaled: false };
    if (tier === 0) return { text: `${secs}s`, scaled: false };
    return { text: `${spellTierTime(secs, tier, castPct).toFixed(2)}s`, scaled: true };
  }
  /** damage/heal range at tier (client base ranges; +dmgPct%/tier linear floor). */
  function rangeAt(r: SpellRow, tier: number): { text: string; scaled: boolean; heal: boolean } | null {
    const [lo, hi, heal] =
      r.dmg_max != null ? [r.dmg_min ?? r.dmg_max, r.dmg_max, false]
      : r.heal_max != null ? [r.heal_min ?? r.heal_max, r.heal_max, true]
      : [null, null, false];
    if (lo == null || hi == null) return null;
    const l = spellTierValue(lo, tier, dmgPct);
    const h = spellTierValue(hi, tier, dmgPct);
    return { text: l === h ? String(h) : `${l}–${h}`, scaled: tier > 0, heal };
  }
  /** damage-per-mana at tier (max damage / tiered mana), when both sides exist. */
  function dpmAt(r: SpellRow, tier: number): string | null {
    if (r.dmg_max == null || r.mana == null || r.mana <= 0) return null;
    const dmg = spellTierValue(r.dmg_max, tier, dmgPct);
    const mana = r.mana >= manaFloor ? spellTierMana(r.mana, tier, manaPct) : r.mana;
    if (mana <= 0) return "∞";
    return (dmg / mana).toFixed(1);
  }

  const ROLES = ["BUFF", "PET_BUFF", "DAMAGE", "CONTROL", "UTILITY", "PET_SUMMON"];

  let spells = $state<SpellRow[]>([]);
  let loading = $state(false);
  let search = $state("");
  let roleFilter = $state("");
  let songsOnly = $state(false);

  // refetch when the SELECTED classes change (debounced). Level 60 = the FULL class
  // lists — power planners borrow buffs other players can cast at any level.
  let gen = 0;
  $effect(() => {
    const classes = [...selClasses];
    const g = ++gen;
    if (classes.length === 0) { spells = []; loading = false; return; }
    loading = true;
    const t = setTimeout(() => {
      querySpells(classes, 60)
        .then((r) => { if (g === gen) { spells = r; loading = false; } })
        .catch((e) => { if (g === gen) { loading = false; s.error = String(e); } });
    }, 250);
    return () => clearTimeout(t);
  });

  interface MergedSpell {
    base: SpellRow;
    entries: { cls: string; level: number; auto: boolean }[];
    minLevel: number;
    anyAuto: boolean;
  }

  // one row per spell id; class column shows "ENC 12 / NEC 12"
  const merged = $derived.by(() => {
    const map = new Map<number, MergedSpell>();
    for (const r of spells) {
      let m = map.get(r.id);
      if (!m) {
        m = { base: r, entries: [], minLevel: r.required_class_level, anyAuto: false };
        map.set(r.id, m);
      }
      m.entries.push({ cls: r.class, level: r.required_class_level, auto: r.is_autogranted });
      m.minLevel = Math.min(m.minLevel, r.required_class_level);
      m.anyAuto = m.anyAuto || r.is_autogranted;
    }
    return [...map.values()].sort(
      (a, b) => a.minLevel - b.minLevel || a.base.name.localeCompare(b.base.name)
    );
  });

  const shown = $derived(
    merged.filter((m) => {
      if (search && !m.base.name.toLowerCase().includes(search.toLowerCase())) return false;
      if (roleFilter && m.base.role !== roleFilter) return false;
      if (songsOnly && !m.base.is_song) return false;
      return true;
    })
  );

  // ---- spell upgrade tiers 0..10 (spell pageid -> tier; scales buff values engine-side) ----
  function tierOf(id: number): number {
    return s.build.spell_tiers?.[id] ?? 0;
  }
  function setSpellTier(id: number, n: number) {
    const clamped = Math.min(10, Math.max(0, n));
    const next = { ...(s.build.spell_tiers ?? {}) };
    if (clamped === 0) delete next[id];
    else next[id] = clamped;
    s.build.spell_tiers = next; // fresh object -> resolve pipeline re-fires
  }
</script>

<div class="classrow">
  {#each allClasses as c (c)}
    <button
      class="chip"
      class:on={selClasses.includes(c)}
      class:build={s.build.classes.includes(c)}
      title={s.build.classes.includes(c) ? `${c} — one of your build's classes` : c}
      onclick={() => toggleClass(c)}
    >{c}</button>
  {/each}
  {#if customized}
    <button class="mini" onclick={resetClasses}>reset to build</button>
  {/if}
</div>

<div class="extbar">
  <span class="extcount">
    <strong>{s.build.external_buffs?.length ?? 0}</strong> external buff{(s.build.external_buffs?.length ?? 0) === 1 ? "" : "s"} cast on you
    {#if plan}· plan uses <strong>{plan.buff_slots_used} / {plan.buff_slot_cap}</strong> slots{/if}
  </span>
  <span class="capctl" title="max simultaneous buffs — engine-enforced; weakest are dropped over the cap">
    <span class="caplbl">Buff cap</span>
    <input
      type="range" min="1" max="60" step="1"
      value={cap}
      oninput={(e) => (capDraft = +e.currentTarget.value)}
      onchange={(e) => commitCap(+e.currentTarget.value)}
    />
    <strong class="capnum">{cap}</strong>
  </span>
</div>

<div class="filters">
  <input class="search" placeholder="search spells…" bind:value={search} />
  <select bind:value={roleFilter}>
    <option value="">(all roles)</option>
    {#each ROLES as r}<option value={r}>{r}</option>{/each}
  </select>
  <label class="songs"><input type="checkbox" bind:checked={songsOnly} /> songs only</label>
  <span class="count">{shown.length} spells (full lists to L60){loading ? " · loading…" : ""}</span>
</div>

<p class="tierlegend">
  Spell tiers: <strong>+{dmgPct}%</strong>/tier damage &amp; healing (linear, floored) ·
  <strong>−{manaPct}%</strong>/tier mana <span class="prov">≈ provisional</span> ·
  <strong>−{castPct}%</strong>/tier cast &amp; reuse ·
  <strong>+{reagentPct}%</strong>/tier reagent conservation.
  Tiered mana/cast values below show the <em>final</em> number at the row's tier.
</p>

<table>
  <thead>
    <tr><th class="casth" title="mark as cast on you by another player">Cast</th><th>Spell</th><th>Class</th><th class="num">Level</th><th>Target</th><th>Role</th><th class="num">Mana</th><th class="num">Cast time</th><th class="num" title="client-extracted base range, tier-scaled (eqlbuilds)">Dmg/Heal</th><th class="num" title="max damage / mana at the row's tier">DPM</th><th>Duration</th><th>Era</th><th class="tierh">Tier</th></tr>
  </thead>
  <tbody>
    {#each shown as m (m.base.id)}
      {@const t = tierOf(m.base.id)}
      {@const mn = manaAt(m.base.mana, t)}
      {@const ct = castAt(m.base.casting_time, t)}
      {@const rg = rangeAt(m.base, t)}
      {@const dpm = dpmAt(m.base, t)}
      {@const ext = externals.has(m.base.id)}
      <tr class:tiered={t > 0} class:extrow={ext}>
        <td class="castcell">
          {#if m.base.is_beneficial}
            {@const canReceive = receivable == null || receivable.has(m.base.id)}
            {#if canReceive}
              <button
                class="castbtn"
                class:on={ext}
                title={ext
                  ? "remove — no longer cast on you"
                  : "cast on you by another player (counts in the buff plan)"}
                onclick={() => toggleExternal(m.base.id)}
              >{ext ? "✓" : "+"}</button>
            {:else}
              <span
                class="nocast"
                title="can't be received from another player (self-only, or not in a known buff line)"
              >–</span>
            {/if}
          {/if}
        </td>
        <td>
          {m.base.name}
          <button class="infobtn" title="spell details & where to obtain" onclick={() => (infoSpell = m.base.id)}>?</button>
          {#if m.anyAuto}<span class="auto" title="autogranted">auto</span>{/if}
          {#if m.base.is_song}<span class="song" title="bard song">song</span>{/if}
        </td>
        <td class="cls">{m.entries.map((e) => `${e.cls} ${e.level}`).join(" / ")}</td>
        <td class="num">{m.minLevel}</td>
        <td class="dim">{m.base.target_type ?? "—"}</td>
        <td class="dim">{m.base.role ?? "—"}</td>
        <td class="num" class:up={mn.scaled} title={mn.scaled ? `base ${m.base.mana} · −${manaPct}%/tier (provisional; low-cost spells may not scale)` : undefined}>{mn.text}</td>
        <td class="num" class:up={ct.scaled} title={ct.scaled ? `base ${m.base.casting_time}s · −${castPct}%/tier` : undefined}>{ct.text}</td>
        <td class="num" class:up={rg?.scaled} class:healv={rg?.heal}
            title={rg ? (m.base.resolved_description ?? undefined) : undefined}>
          {rg ? (rg.heal ? `+${rg.text}` : rg.text) : "—"}
        </td>
        <td class="num dpm" class:up={t > 0 && dpm != null}>{dpm ?? "—"}</td>
        <td class="dim">{m.base.duration ?? "—"}</td>
        <td class="dim">{m.base.era ?? "—"}</td>
        <td class="tiercell">
          <button class="tbtn" onclick={() => setSpellTier(m.base.id, tierOf(m.base.id) - 1)} disabled={tierOf(m.base.id) <= 0}>−</button>
          <span class="tval" class:on={tierOf(m.base.id) > 0}>{tierOf(m.base.id)}</span>
          <button class="tbtn" onclick={() => setSpellTier(m.base.id, tierOf(m.base.id) + 1)} disabled={tierOf(m.base.id) >= 10}>+</button>
        </td>
      </tr>
    {/each}
  </tbody>
</table>

{#if infoSpell != null}
  <SpellInfoPopup {s} spellId={infoSpell} onclose={() => (infoSpell = null)} />
{/if}

<style>
  /* ---- class buttons: all 16, build classes ringed gold ---- */
  .classrow { display: flex; gap: .35rem; align-items: center; flex-wrap: wrap; margin-bottom: .5rem; }
  .chip.build { box-shadow: 0 0 0 1px #8a7440; }
  .mini {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 2px 8px; cursor: pointer; font-size: .72rem;
  }
  .mini:hover { color: #e6e6e6; border-color: #c9b26a; }

  /* ---- external buffs bar + cap slider ---- */
  .extbar {
    display: flex; gap: 1rem; align-items: center; flex-wrap: wrap;
    background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
    padding: .35rem .7rem; margin-bottom: .5rem;
  }
  .extcount { color: #9ab; font-size: .82rem; }
  .extcount strong { color: #fa8; }
  .capctl { display: flex; gap: .45rem; align-items: center; margin-left: auto; }
  .caplbl { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .06em; }
  .capnum { color: #fc6; min-width: 1.6rem; text-align: right; }

  /* ---- cast-on-me column ---- */
  .casth { text-align: center; }
  .castcell { text-align: center; }
  .castbtn {
    width: 20px; height: 20px; line-height: 1; padding: 0;
    background: #171a20; color: #667; border: 1px solid #333; border-radius: 4px;
    cursor: pointer; font-size: .8rem;
  }
  .castbtn:hover { color: #fa8; border-color: #fa8; }
  .castbtn.on { background: #2a1f16; color: #fa8; border-color: #fa8; }
  .nocast { color: #445; cursor: help; }
  .infobtn {
    margin-left: .3rem; width: 16px; height: 16px; line-height: 1; padding: 0;
    background: #171a20; color: #6ac; border: 1px solid #345; border-radius: 50%;
    cursor: pointer; font-size: .68rem; vertical-align: middle;
  }
  .infobtn:hover { color: #9cf; border-color: #6ac; }
  tr.extrow td { background: rgba(250, 168, 136, .06); }

  .filters { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin-bottom: .6rem; }
  .search { flex: 1; min-width: 180px; padding: 5px 8px; background: #1c1f26; border: 1px solid #333; color: #e6e6e6; border-radius: 6px; }
  .chip { background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px; padding: 2px 8px; cursor: pointer; font-size: .8rem; }
  .chip.on { background: #2a6; color: #012; border-color: #2a6; font-weight: 600; }
  select { background: #1c1f26; color: #e6e6e6; border: 1px solid #333; border-radius: 6px; padding: 3px 6px; }
  .songs { color: #9ab; display: flex; gap: .3rem; align-items: center; font-size: .85rem; }
  .count { color: #667; font-size: .8rem; margin-left: auto; }
  table { border-collapse: collapse; width: 100%; font-size: .82rem; }
  th { text-align: left; color: #89a; font-weight: 600; padding: 3px 8px 3px 0; border-bottom: 1px solid #333; position: sticky; top: 0; background: #14161a; }
  th.num { text-align: right; }
  td { padding: 2px 8px 2px 0; border-bottom: 1px solid #20242b; }
  td.num { text-align: right; color: #fc6; }
  .cls { color: #9ab; }
  .dim { color: #89a; }
  .auto { margin-left: .4rem; font-size: .65rem; background: #2a6; color: #012; border-radius: 4px; padding: 0 4px; vertical-align: middle; }
  .song { margin-left: .3rem; font-size: .65rem; background: #46c; color: #dee; border-radius: 4px; padding: 0 4px; vertical-align: middle; }
  tr.tiered td { background: rgba(201, 178, 106, .07); }
  .tierh { text-align: center; }
  .tiercell { white-space: nowrap; text-align: center; }
  .tbtn {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 4px;
    padding: 0 6px; cursor: pointer; font: inherit; font-size: .75rem; line-height: 1.25;
  }
  .tbtn:hover:not(:disabled) { color: #e6e6e6; border-color: #c9b26a; }
  .tbtn:disabled { opacity: .3; cursor: default; }
  .tval { display: inline-block; min-width: 1.3rem; text-align: center; color: #667; }
  .tval.on { color: #c9b26a; font-weight: 600; }
  /* tier-scaled finals: green, matching the equipment windows' convention */
  td.num.up { color: #5d5; font-weight: 600; }
  td.healv { color: #6c9; }
  td.dpm { color: #d86; }
  .tierlegend { color: #987; font-size: .74rem; margin: 0 0 .5rem; }
  .tierlegend strong { color: #c9b26a; }
  .tierlegend .prov { color: #da5; font-size: .68rem; }
</style>
