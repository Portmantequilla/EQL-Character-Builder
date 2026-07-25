<script lang="ts">
  import type { AppState } from "../state.svelte";
  import type { AaAbility } from "../api";
  import { aaCost, aaRank, fmtAaCost, setAaRank } from "../aa";

  let { s }: { s: AppState } = $props();

  // ---- header: points + the engine's plan ----
  const plan = $derived(s.result?.aa_plan ?? null);
  const spent = $derived(plan?.points_spent ?? 0);
  const available = $derived(s.build.aa_points_available ?? 0);
  const overspent = $derived(spent > available);

  /** number inputs hand back NaN when emptied — never let that reach the u32 on the Rust side */
  function setPoints(n: number) {
    s.build.aa_points_available = Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0;
  }

  // ---- filters ----
  let search = $state("");
  let hideMaxed = $state(false);
  let onlyPurchased = $state(false);
  const q = $derived(search.trim().toLowerCase());

  function rankOf(aa: AaAbility): number {
    return aaRank(s.build, aa.id);
  }
  function bump(aa: AaAbility, delta: number) {
    setAaRank(s.build, aa, rankOf(aa) + delta);
  }
  function needsLevel(aa: AaAbility): boolean {
    return aa.required_level != null && aa.required_level > s.build.level;
  }
  /** the wiki left ranks blank -> the max-out cost is a floor, not a number */
  function unknownRanks(aa: AaAbility): boolean {
    return !aa.cost_complete || aaCost(aa, aa.max_rank).unknown > 0;
  }

  /** search + the two toggles, applied to one section's list */
  function shown(list: AaAbility[]): AaAbility[] {
    return list.filter((aa) => {
      if (q && !aa.name.toLowerCase().includes(q)) return false;
      const r = rankOf(aa);
      if (onlyPurchased && r === 0) return false;
      if (hideMaxed && r >= aa.max_rank) return false;
      return true;
    });
  }
  function purchasedIn(list: AaAbility[]): number {
    return list.filter((aa) => rankOf(aa) > 0).length;
  }

  // ---- category sections ----
  const general = $derived(s.aas.filter((a) => a.category === "GENERAL"));
  const archetype = $derived(s.aas.filter((a) => a.category === "ARCHETYPE"));
  const special = $derived(s.aas.filter((a) => a.category === "SPECIAL"));

  /** CLASS AAs, grouped under the build's own classes (the engine gates them the same way).
      Deduped: a build may hold the same class more than once, and the keyed each below
      hard-throws on duplicate keys (one AA group per distinct class is also just correct). */
  const classGroups = $derived(
    [...new Set(s.build.classes)].map((cls) => ({
      cls,
      list: s.aas.filter(
        (a) => a.category === "CLASS" && (a.class_abbr?.toUpperCase() ?? "") === cls.toUpperCase()
      ),
    }))
  );

  const loaded = $derived(s.aas.length > 0);
</script>

{#snippet aaTable(list: AaAbility[])}
  {@const rows = shown(list)}
  {#if rows.length === 0}
    <p class="none">no AAs match the current filter</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Ability</th>
          <th class="mid">Rank</th>
          <th class="num">Cost</th>
          <th class="num">Max</th>
          <th>Description</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as aa (aa.id)}
          {@const rank = rankOf(aa)}
          <tr class:bought={rank > 0}>
            <td class="name">
              <span class="aaname" class:on={rank > 0}>{aa.name}</span>
              {#if needsLevel(aa)}
                <span class="lvlbadge" title="purchasable, but not usable until level {aa.required_level}">
                  needs level {aa.required_level}
                </span>
              {/if}
            </td>
            <td class="mid">
              <span class="stepper">
                <button class="mini" aria-label="lower rank of {aa.name}"
                        onclick={() => bump(aa, -1)} disabled={rank <= 0}>−</button>
                <strong class="rnum" class:on={rank > 0}>{rank}</strong>
                <span class="rmax">/ {aa.max_rank}</span>
                <button class="mini" aria-label="raise rank of {aa.name}"
                        onclick={() => bump(aa, 1)} disabled={rank >= aa.max_rank}>+</button>
              </span>
            </td>
            <td class="num cost" class:on={rank > 0}>{fmtAaCost(aa, rank)}</td>
            <td class="num maxcost" title={unknownRanks(aa)
              ? "the wiki lists '?' for some ranks — this total is a lower bound"
              : "points to take every rank"}>
              {fmtAaCost(aa, aa.max_rank)}
            </td>
            <td class="desc">{aa.description}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
{/snippet}

{#if !loaded}
  <p class="muted">No AA data — the wiki mirror has no <code>aa</code> table yet.</p>
{:else}
  <div class="hdr">
    <label class="pts" for="aapoints">AA points available</label>
    <input
      id="aapoints" class="ptsin" type="number" min="0" max="9999" step="1"
      value={s.build.aa_points_available}
      oninput={(e) => setPoints(e.currentTarget.valueAsNumber)}
    />
    <span class="spent" class:over={overspent}>
      Spent: <strong>{spent}{plan?.cost_is_lower_bound ? "+" : ""}</strong> / {available}
    </span>
    {#if overspent}
      <span class="overtag">over budget</span>
    {/if}
    <span class="spacer"></span>
    <span class="count">{s.aas.length} AAs · {purchasedIn(s.aas)} purchased</span>
  </div>

  {#if plan?.cost_is_lower_bound}
    <p class="lowerbound">
      cost is a lower bound — the wiki lists “?” for some ranks of what you picked
    </p>
  {/if}

  {#if (plan?.level_locked.length ?? 0) > 0 || (plan?.class_locked.length ?? 0) > 0}
    <div class="chips">
      {#each plan?.level_locked ?? [] as w (w)}
        <span class="chip lvl" title="kept, but not usable yet">{w}</span>
      {/each}
      {#each plan?.class_locked ?? [] as w (w)}
        <span class="chip cls" title="kept, but not usable yet">{w} — not granted by your classes</span>
      {/each}
      <span class="chiphint">kept, but not usable yet</span>
    </div>
  {/if}

  <div class="filters">
    <input class="search" placeholder="filter AAs by name…" bind:value={search} />
    <label class="chk">
      <input type="checkbox" bind:checked={hideMaxed} />
      hide maxed
    </label>
    <label class="chk">
      <input type="checkbox" bind:checked={onlyPurchased} />
      show only purchased
    </label>
    {#if q || hideMaxed || onlyPurchased}
      <button class="mini" onclick={() => { search = ""; hideMaxed = false; onlyPurchased = false; }}>
        clear filters
      </button>
    {/if}
  </div>

  <section>
    <h3>General <span class="dim">({general.length})</span></h3>
    {@render aaTable(general)}
  </section>

  <section>
    <h3>Archetype <span class="dim">({archetype.length})</span></h3>
    <p class="honest">
      The wiki does not publish which class combos grant which Archetype AAs — all are listed.
    </p>
    {@render aaTable(archetype)}
  </section>

  <section>
    <h3>Class <span class="dim">({s.build.classes.join(" / ") || "no classes picked"})</span></h3>
    {#if s.build.classes.length === 0}
      <p class="none">pick a class in the header to see its AAs</p>
    {:else}
      {#each classGroups as g (g.cls)}
        <h4>{g.cls} <span class="dim">({g.list.length})</span></h4>
        {#if g.list.length === 0}
          <p class="none">the wiki lists no Class AAs for {g.cls}</p>
        {:else}
          {@render aaTable(g.list)}
        {/if}
      {/each}
    {/if}
  </section>

  <section>
    <h3>Special <span class="dim">({special.length})</span></h3>
    {@render aaTable(special)}
  </section>
{/if}

<style>
  .muted { color: #667; }
  .spacer { flex: 1; }

  /* ---- header ---- */
  .hdr {
    display: flex; gap: .55rem; align-items: center; flex-wrap: wrap;
    padding: .45rem .65rem; margin-bottom: .5rem;
    background: linear-gradient(#1a1712, #15130f);
    border: 1px solid #5a4326; border-radius: 8px;
  }
  .pts { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .ptsin {
    width: 80px; padding: 3px 7px; font: inherit; font-size: .85rem;
    background: #1c1f26; color: #e8d9a8; border: 1px solid #8a7440; border-radius: 6px;
  }
  .ptsin:focus { outline: none; border-color: #c9b26a; }
  .spent { color: #89a; font-size: .85rem; }
  .spent strong { color: #e8d9a8; }
  .spent.over, .spent.over strong { color: #f66; font-weight: 600; }
  .overtag {
    color: #f88; font-size: .7rem; border: 1px solid #a33; background: #2a1a1a;
    border-radius: 6px; padding: 1px 6px;
  }
  .count { color: #667; font-size: .75rem; }

  .lowerbound {
    color: #e0a44c; font-size: .75rem; margin: 0 0 .5rem;
    background: #241d10; border: 1px solid #6b4f1e; border-radius: 6px;
    padding: .3rem .55rem; max-width: 760px;
  }

  .chips { display: flex; gap: 5px; align-items: center; flex-wrap: wrap; margin-bottom: .5rem; }
  .chip { font-size: .72rem; border-radius: 6px; padding: 1px 7px; }
  .chip.lvl { background: #2a2010; color: #e8a24c; border: 1px solid #8a5f22; }
  .chip.cls { background: #2a1a1a; color: #e07070; border: 1px solid #8a3a3a; }
  .chiphint { color: #8a7f66; font-size: .7rem; }

  /* ---- filters ---- */
  .filters { display: flex; gap: .6rem; align-items: center; flex-wrap: wrap; margin-bottom: .7rem; }
  .search {
    width: 260px; padding: 5px 8px; background: #1c1f26; border: 1px solid #333;
    color: #e6e6e6; border-radius: 6px; font: inherit; font-size: .85rem;
  }
  .chk {
    display: flex; align-items: center; gap: 4px;
    color: #cbb27a; font-size: .8rem; cursor: pointer; user-select: none;
  }
  .chk input { accent-color: #c9b26a; cursor: pointer; margin: 0; }

  /* ---- sections ---- */
  section { margin-bottom: 1.1rem; }
  h3 {
    margin: 0 0 .3rem; color: #e8d9a8; font-size: .95rem;
    font-variant: small-caps; letter-spacing: .06em;
    border-bottom: 1px solid #3a2f1a; padding-bottom: .15rem;
  }
  h4 {
    margin: .55rem 0 .2rem; color: #c9b26a; font-size: .8rem;
    font-variant: small-caps; letter-spacing: .08em;
  }
  .dim { color: #8a7f66; font-size: .78rem; font-variant: normal; letter-spacing: normal; }
  .honest { color: #8a7f66; font-size: .74rem; margin: 0 0 .35rem; max-width: 760px; }
  .none { color: #667; font-size: .78rem; font-style: italic; margin: .2rem 0 .4rem; }

  /* ---- table (Stats-tab conventions: dense, right-aligned numbers) ---- */
  table { border-collapse: collapse; font-size: .85rem; width: 100%; max-width: 1100px; }
  th {
    text-align: left; color: #89a; font-weight: 600;
    padding: 3px 14px 3px 0; border-bottom: 1px solid #333; white-space: nowrap;
  }
  th.num, td.num { text-align: right; }
  th.mid, td.mid { text-align: center; }
  td { padding: 3px 14px 3px 0; border-bottom: 1px solid #20242b; vertical-align: middle; }
  tr.bought { background: rgba(201, 178, 106, .06); }

  .name { white-space: nowrap; }
  .aaname { color: #9ab; font-weight: 600; }
  .aaname.on { color: #e8d9a8; }
  .lvlbadge {
    margin-left: 6px; font-size: .66rem; color: #f0a24c;
    border: 1px solid #8a5f22; background: #241d10; border-radius: 6px; padding: 0 5px;
    white-space: nowrap;
  }

  .stepper { display: inline-flex; gap: 4px; align-items: center; white-space: nowrap; }
  .mini {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 0 8px; cursor: pointer; font: inherit; font-size: .8rem; line-height: 1.5;
  }
  .mini:hover:not(:disabled) { color: #e6e6e6; border-color: #2a6; }
  .mini:disabled { opacity: .3; cursor: default; }
  .rnum { color: #667; min-width: 1rem; text-align: right; }
  .rnum.on { color: #e8d9a8; }
  .rmax { color: #667; font-size: .75rem; }

  .cost { color: #667; }
  .cost.on { color: #c9b26a; font-weight: 600; }
  .maxcost { color: #7d8496; font-size: .78rem; cursor: help; }

  .desc { color: #7d8496; font-size: .75rem; line-height: 1.35; min-width: 260px; }
</style>
