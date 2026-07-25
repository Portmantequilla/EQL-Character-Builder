<script lang="ts">
  import type { AppState } from "../state.svelte";
  import { orderStatKeys } from "../format";

  let { s }: { s: AppState } = $props();

  const r = $derived(s.result);
  const statKeys = $derived(r ? orderStatKeys(Object.keys(r.stats)) : []);

  // ---- starting attributes (client-validated tables: race base + exactly 30 points
  // per class, additive — eqltools.com Attributes) ----
  const ATTRS = ["STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA"] as const;
  const classes3 = $derived(s.build.classes.slice(0, 3));
  const raceRow = $derived(
    s.build.race != null ? (s.staticData?.race_base_stats?.[s.build.race] ?? null) : null
  );
  const classMods = $derived(
    classes3.map((c) => ({
      abbr: c,
      mods: s.staticData?.class_stat_mods?.[c.toUpperCase()] ?? {},
    }))
  );
  const ceiling = $derived(s.staticData?.stat_naked_ceiling ?? 150);
  function startTotal(k: string): number {
    let t = raceRow?.[k] ?? 0;
    for (const cm of classMods) t += cm.mods[k] ?? 0;
    return t;
  }
</script>

{#if !r}
  <p class="muted">Waiting for first calculation…</p>
{:else}
  <div class="cards">
    <div class="card">
      <h3>Build</h3>
      <p class="big">{r.classes.join(" / ") || "no classes"}</p>
      <p>{r.race ?? "any race"} · level {r.level}</p>
    </div>

    <div class="card">
      <h3>Wearable gear</h3>
      <p class="big">{r.wearable_item_count.toLocaleString()}</p>
      <p>items usable by this class combo</p>
    </div>

    <div class="card">
      <h3>Buff slots</h3>
      <p class="big">{r.buff_plan.buff_slots_used} / {r.buff_plan.buff_slot_cap}</p>
      <p>{r.buff_plan.active.length} active · {r.buff_plan.rejected.length} rejected</p>
    </div>

    <div class="card">
      <h3>Pet</h3>
      {#if r.pet}
        <p class="big">{r.pet.summon.name}</p>
        <p>
          {#if r.pet.calculated_level != null}L{r.pet.calculated_level}{:else}level unknown{/if}
          · tier {s.build.pet_summon_tier}
        </p>
      {:else}
        <p class="muted">no pet summon selected</p>
      {/if}
    </div>

    {#if classes3.length === 3}
      <div class="card wide">
        <h3>Starting attributes</h3>
        <table class="startattrs">
          <thead>
            <tr>
              <th></th>
              <th class="num">{s.build.race ?? "race?"}</th>
              <!-- key includes the index: a build may hold the same class more than once,
                   and duplicate keys hard-throw (three +MAG columns is the honest display) -->
              {#each classMods as cm, ci (cm.abbr + "#" + ci)}<th class="num">+{cm.abbr}</th>{/each}
              <th class="num">Starting</th>
            </tr>
          </thead>
          <tbody>
            {#each ATTRS as k (k)}
              {@const total = startTotal(k)}
              <tr>
                <td class="key">{k}</td>
                <td class="num race">{raceRow?.[k] ?? "—"}</td>
                {#each classMods as cm, ci (cm.abbr + "#" + ci)}
                  <td class="num mod">{(cm.mods[k] ?? 0) > 0 ? `+${cm.mods[k]}` : "·"}</td>
                {/each}
                <td class="num total" class:overceil={raceRow != null && total > ceiling}>
                  {raceRow != null ? total : "—"}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if raceRow == null}
          <p class="muted small">pick a race to complete the column — each class adds exactly 30 points</p>
        {:else if ATTRS.some((k) => startTotal(k) > ceiling)}
          <p class="ceilnote">values over {ceiling} exceed the player-reported naked ceiling</p>
        {/if}
        <p class="muted small">client-validated: race base + 30 points per class, additive (eqltools.com)</p>
      </div>
    {/if}

    <div class="card wide">
      <h3>Active buffs ({r.buff_plan.active.length}) — included in the stat totals</h3>
      {#if r.buff_plan.active.length === 0}
        <p class="muted">none — the Buffs tab auto-selects your strongest self-cast lines</p>
      {:else}
        {@const top = [...r.buff_plan.active].sort((a, b) => b.total_value - a.total_value)}
        <div class="buffscroll">
          <ul class="buffs">
            {#each top as b (b.name)}
              <li>
                <span class="bname">{b.name}</span>
                <span class="bval">+{b.total_value}</span>
                <span class="btag t-{b.source_tag.toLowerCase()}">{b.source_tag.toLowerCase()}</span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>

    <div class="card wide">
      <h3>Stats w/ equipment and buffs</h3>
      <table>
        <thead>
          <tr>
            <th></th><th class="num hd">Equip</th><th class="num hd">Buffs</th><th class="num hd">Total</th>
          </tr>
        </thead>
        <tbody>
          {#each statKeys as k}
            {@const l = r.stats[k]}
            {@const eq = l.equipment + l.tier_bonus + Math.round(l.item_effects)}
            <tr>
              <td class="key">{k}</td>
              <td class="num equipn">{eq === 0 ? "·" : `+${eq}`}</td>
              <td class="num buffn">{l.buffs === 0 ? "·" : `+${Math.round(l.buffs)}`}</td>
              <td class="num">{l.capped_total}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="muted small">haste: {r.equipment_haste_pct}% equipment · {r.buff_haste_pct}% buffs</p>
    </div>

    <div class="card wide">
      <h3>Equipment warnings ({r.equipment_warnings.length})</h3>
      {#if r.equipment_warnings.length === 0}
        <p class="muted">none — all equipped gear is usable</p>
      {:else}
        <ul class="warns">
          {#each r.equipment_warnings as w}
            <li class:inactive={w.status === "SAVED_INACTIVE"}>
              <span class="slot">{w.slot}</span>
              <span class="item">{w.item}</span>
              <span class="reason">{w.reason}</span>
              <span class="status">{w.status}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="card wide">
      <h3>Engine notes</h3>
      {#if r.notes.length === 0}
        <p class="muted">none</p>
      {:else}
        <ul class="notes">{#each r.notes as n}<li>{n}</li>{/each}</ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: .8rem; }
  .card { background: #1c1f26; border: 1px solid #262b33; border-radius: 8px; padding: .6rem .8rem; }
  .card.wide { grid-column: span 2; }
  h3 { margin: 0 0 .3rem; font-size: .75rem; text-transform: uppercase; letter-spacing: .05em; color: #89a; }
  p { margin: .15rem 0; }
  .big { font-size: 1.15rem; font-weight: 600; }
  .muted { color: #667; }
  .small { font-size: .72rem; }
  table { border-collapse: collapse; width: 100%; font-size: .8rem; }
  td { padding: 1px 6px 1px 0; border-bottom: 1px solid #20242b; }
  .key { color: #9ab; }
  .num { text-align: right; color: #fc6; font-weight: 600; }
  ul { list-style: none; padding: 0; margin: 0; }
  .warns li { display: flex; gap: .6rem; align-items: baseline; padding: 2px 0; border-bottom: 1px solid #20242b; color: #ea8; }
  .warns li.inactive { color: #f66; }
  .warns .slot { color: #89a; font-size: .75rem; min-width: 80px; }
  .warns .item { flex: 1; }
  .warns .reason { font-size: .72rem; }
  .warns .status { font-size: .68rem; border: 1px solid currentColor; border-radius: 4px; padding: 0 4px; }
  .notes li { padding: 2px 0; border-bottom: 1px solid #20242b; color: #9ab; font-size: .8rem; }
  @media (max-width: 1100px) { .cards { grid-template-columns: repeat(2, 1fr); } }

  /* ---- starting attributes (race + 3×30 class points) ---- */
  .startattrs th { text-align: right; color: #89a; font-weight: 600; font-size: .72rem;
                   padding: 1px 6px 2px 0; border-bottom: 1px solid #333; }
  .startattrs th:first-child { text-align: left; }
  .startattrs .race { color: #9ab; }
  .startattrs .mod { color: #6c9; }
  .startattrs .total { color: #fc6; font-weight: 600; }
  .startattrs .total.overceil { color: #f90; }
  .ceilnote { color: #f90; font-size: .72rem; margin: .2rem 0 0; }

  /* ---- active buffs card ---- */
  .buffscroll { max-height: 230px; overflow-y: auto; padding-right: 4px; }
  .buffscroll::-webkit-scrollbar { width: 8px; }
  .buffscroll::-webkit-scrollbar-track { background: #12151c; border-radius: 4px; }
  .buffscroll::-webkit-scrollbar-thumb { background: #3a3f4a; border-radius: 4px; }
  .buffscroll::-webkit-scrollbar-thumb:hover { background: #8a7440; }
  .hd { font-size: .68rem; }
  .equipn { color: #6c9; }
  .buffn { color: #6af; }
  .buffs { list-style: none; margin: 0; padding: 0; }
  .buffs li { display: flex; gap: .6rem; align-items: baseline; padding: 1px 0; border-bottom: 1px solid #20242b; font-size: .8rem; }
  .bname { flex: 1; color: #cbd; }
  .bval { color: #fc6; font-weight: 600; }
  .btag { font-size: .62rem; border: 1px solid currentColor; border-radius: 4px; padding: 0 4px; font-variant: small-caps; }
  .t-auto { color: #6c9; } .t-manual { color: #c9b26a; } .t-other { color: #c9c; }
  .t-external { color: #fa8; } .t-bard { color: #6af; }
</style>
