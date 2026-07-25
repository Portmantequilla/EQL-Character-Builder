<script lang="ts">
  import type { AppState } from "../state.svelte";
  import { farmList, type FarmSource } from "../api";
  import ItemPicker from "../ItemPicker.svelte";

  let { s }: { s: AppState } = $props();

  let sources = $state<FarmSource[]>([]);
  let loading = $state(false);
  let showPicker = $state(false);

  // names of currently equipped items — from the RESULT's class-filter-independent
  // map (the browser list is wearable-filtered and drops SAVED_INACTIVE items)
  const equippedNames = $derived.by(() => {
    const m = s.result?.equipped_item_names ?? {};
    return [...new Set(Object.values(m))];
  });
  const allNames = $derived([...new Set([...equippedNames, ...s.wishlist])]);

  let gen = 0;
  $effect(() => {
    const names = [...allNames];
    const g = ++gen;
    if (names.length === 0) { sources = []; loading = false; return; }
    loading = true;
    const t = setTimeout(() => {
      farmList(names)
        .then((r) => { if (g === gen) { sources = r; loading = false; } })
        .catch((e) => { if (g === gen) { loading = false; s.error = String(e); } });
    }, 200);
    return () => clearTimeout(t);
  });

  interface MobGroup { mob: string; level: string | null; drops: { item: string; rarity: string | null }[]; }
  interface ZoneGroup { zone: string; mobs: MobGroup[]; }

  // zone header -> mobs (level, rarity) -> items
  const byZone = $derived.by(() => {
    const zones = new Map<string, Map<string, MobGroup>>();
    for (const src of sources) {
      const z = src.zone ?? "Unknown zone";
      let mobs = zones.get(z);
      if (!mobs) { mobs = new Map(); zones.set(z, mobs); }
      let mob = mobs.get(src.mob);
      if (!mob) { mob = { mob: src.mob, level: src.mob_level, drops: [] }; mobs.set(src.mob, mob); }
      mob.drops.push({ item: src.item, rarity: src.rarity });
    }
    const out: ZoneGroup[] = [...zones.entries()].map(([zone, mobs]) => ({
      zone,
      mobs: [...mobs.values()].sort((a, b) => a.mob.localeCompare(b.mob)),
    }));
    return out.sort((a, b) => a.zone.localeCompare(b.zone));
  });

  // requested items with zero drop sources (vendor/quest items)
  const noSource = $derived.by(() => {
    const found = new Set(sources.map((x) => x.item.toLowerCase()));
    return allNames.filter((n) => !found.has(n.toLowerCase()));
  });

  function addToWishlist(name: string) {
    if (!s.wishlist.includes(name)) s.wishlist = [...s.wishlist, name];
    showPicker = false;
  }
  function removeFromWishlist(name: string) {
    s.wishlist = s.wishlist.filter((n) => n !== name);
  }
</script>

<div class="head">
  <span class="summary">
    Farming <strong>{allNames.length}</strong> items
    ({equippedNames.length} equipped + {s.wishlist.length} wishlist)
    {#if loading}· loading…{/if}
  </span>
  <button class="mini" onclick={() => (showPicker = !showPicker)}>
    {showPicker ? "Close" : "+ Add item"}
  </button>
</div>

{#if s.wishlist.length > 0}
  <div class="wishlist">
    {#each s.wishlist as n (n)}
      <span class="wish">{n}<button onclick={() => removeFromWishlist(n)} title="remove">×</button></span>
    {/each}
  </div>
{/if}

{#if showPicker}
  <div class="pickerbox">
    <ItemPicker
      items={s.items}
      level={s.build.level}
      onpick={(i) => addToWishlist(i.name)}
      placeholder="search any wearable item…"
    />
  </div>
{/if}

{#if allNames.length === 0}
  <p class="muted">Nothing to farm — equip gear or add items to the wishlist.</p>
{:else}
  {#each byZone as z (z.zone)}
    <section>
      <h2>{z.zone}</h2>
      {#each z.mobs as m (m.mob)}
        <div class="mob">
          <span class="mobname">{m.mob}</span>
          {#if m.level}<span class="lvl">L{m.level}</span>{/if}
          <ul>
            {#each m.drops as d}
              <li>
                <span class="item">{d.item}</span>
                {#if d.rarity}<span class="rarity">{d.rarity}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </section>
  {/each}

  {#if noSource.length > 0 && !loading}
    <section>
      <h2 class="nosrc">No drop sources found</h2>
      <ul class="nolist">
        {#each noSource as n (n)}
          <li>{n} <span class="dim">— vendor/quest item?</span></li>
        {/each}
      </ul>
    </section>
  {/if}
{/if}

<style>
  .head { display: flex; gap: .8rem; align-items: center; margin-bottom: .5rem; }
  .summary { color: #9ab; }
  .summary strong { color: #fc6; }
  .mini { background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px; padding: 2px 9px; cursor: pointer; }
  .mini:hover { color: #e6e6e6; border-color: #2a6; }
  .wishlist { display: flex; gap: .35rem; flex-wrap: wrap; margin-bottom: .5rem; }
  .wish { background: #22262d; border: 1px solid #333; border-radius: 6px; padding: 1px 4px 1px 8px; font-size: .8rem; color: #cab; }
  .wish button { background: none; border: none; color: #a66; cursor: pointer; font-size: .85rem; padding: 0 4px; }
  .wish button:hover { color: #f88; }
  .pickerbox { margin: .3rem 0 .7rem; padding: .5rem; background: #171a20; border: 1px solid #2a2f38; border-radius: 8px; max-width: 700px; }
  .muted { color: #667; }
  section { margin-bottom: .9rem; }
  h2 { font-size: .95rem; color: #6c9; margin: .7rem 0 .3rem; border-bottom: 1px solid #2a2f38; padding-bottom: 2px; }
  h2.nosrc { color: #c86; }
  .mob { display: flex; gap: .6rem; align-items: baseline; padding: 2px 0 2px .6rem; border-bottom: 1px solid #20242b; }
  .mobname { min-width: 220px; }
  .lvl { color: #89a; font-size: .75rem; min-width: 60px; }
  ul { list-style: none; padding: 0; margin: 0; flex: 1; display: flex; gap: .8rem; flex-wrap: wrap; }
  .item { color: #fc6; font-size: .8rem; }
  .rarity { color: #c9c; font-size: .7rem; margin-left: .25rem; }
  .nolist { display: block; }
  .nolist li { padding: 2px 0 2px .6rem; border-bottom: 1px solid #20242b; font-size: .85rem; }
  .dim { color: #667; font-size: .75rem; }
</style>
