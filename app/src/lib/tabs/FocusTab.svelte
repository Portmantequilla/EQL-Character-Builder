<script lang="ts">
  // FOCUS EFFECTS — informational reference (user request 2026-07-19, formatted after
  // their curated guide): pick a tier, see every focus effect at that tier with the
  // items that carry it, class restrictions, and where each item drops. Item names
  // link to their eqlwiki page. Data: item_effect FOCUS rows + drops/mobs joins;
  // descriptions come from the focus spells' own wiki pages.
  import type { AppState } from "../state.svelte";
  import { focusEffects, openUrl, type FocusEffectRow } from "../api";
  import { eraAllowed } from "../format";

  let { s }: { s: AppState } = $props();

  let rows = $state<FocusEffectRow[] | null>(null);
  $effect(() => {
    focusEffects().then((r) => (rows = r)).catch((e) => (s.error = String(e)));
  });

  const TIERS = ["I", "II", "III", "Pet"] as const;
  type Tier = (typeof TIERS)[number];
  let tier = $state<Tier>("I");

  /** trailing Roman numeral = the tier; untiered focuses (Minion/Servant of X pet
   *  summon focuses) group under "Pet". */
  function tierOf(effect: string): Tier | null {
    const m = /\s(I{1,3})$/.exec(effect);
    if (m) return m[1] as Tier;
    return /^(Minion|Servant) of /.test(effect) ? "Pet" : null;
  }

  // short family fallbacks (the user's guide wording) when the spell page has no text
  const FALLBACK: Record<string, string> = {
    "Affliction Efficiency": "Reduces mana cost of damage-over-time spells.",
    "Affliction Haste": "Reduces cast time of detrimental duration spells.",
    "Burning Affliction": "Increases DoT damage.",
    "Enhancement Haste": "Reduces cast time of beneficial duration spells.",
    "Extended Enhancement": "Increases beneficial spell duration.",
    "Extended Range": "Increases spell range.",
    "Improved Damage": "Increases direct damage spell damage.",
    "Improved Healing": "Increases healing spell effectiveness.",
    "Mana Preservation": "Reduces mana cost of spells.",
    "Reagent Conservation": "Chance to save spell components.",
    "Reanimation Efficiency": "Reduces mana cost of summon undead spells.",
    "Reanimation Haste": "Reduces cast time of summon undead spells.",
    "Spell Haste": "Reduces cast time of spells.",
    "Summoning Efficiency": "Reduces mana cost of summon creature spells.",
    "Summoning Haste": "Reduces cast time of summon creature spells.",
  };
  function describe(effect: string, desc: string | null): string {
    if (desc) return desc;
    const family = effect.replace(/\s+I{1,3}$/, "");
    return FALLBACK[family] ?? "";
  }

  interface Group {
    effect: string;
    description: string;
    items: FocusEffectRow[];
  }
  const groups = $derived.by(() => {
    const map = new Map<string, Group>();
    for (const r of rows ?? []) {
      if (tierOf(r.effect_name) !== tier) continue;
      let g = map.get(r.effect_name);
      if (!g) {
        g = { effect: r.effect_name, description: describe(r.effect_name, r.description), items: [] };
        map.set(r.effect_name, g);
      }
      g.items.push(r);
    }
    return [...map.values()].sort((a, b) => a.effect.localeCompare(b.effect));
  });

  /** classes chip: the wiki's ALL stays ALL; else the class list joined. */
  function classChip(classes: string[]): string {
    if (classes.length === 0 || classes.includes("ALL")) return "ALL";
    return classes.join(" ");
  }
  /** the user's source format: mobs sharing a zone fold to "A / B / C - Zone". */
  function foldSources(sources: [string, string | null][]): string[] {
    const byZone = new Map<string, string[]>();
    for (const [mob, zone] of sources) {
      const z = zone ?? "?";
      (byZone.get(z) ?? byZone.set(z, []).get(z)!).push(mob);
    }
    return [...byZone.entries()].map(([z, mobs]) => `${mobs.join(" / ")} - ${z}`);
  }
  function wikiUrl(name: string): string {
    return `https://eqlwiki.com/${name.replace(/ /g, "_")}`;
  }
</script>

<div class="tierrow">
  {#each TIERS as t (t)}
    <button class="tbtn" class:on={tier === t} onclick={() => (tier = t)}>
      {t === "Pet" ? "Pet focuses" : `Level ${t}`}
    </button>
  {/each}
  <span class="hint">
    informational — item names open eqlwiki.com; sources come from the drop data
  </span>
</div>

{#if rows == null}
  <p class="muted">loading…</p>
{:else}
  <h2 class="banner">
    {tier === "Pet" ? "PET SUMMON FOCUS EFFECTS" : `LEVEL ${tier} FOCUS EFFECTS`}
  </h2>

  {#each groups as g (g.effect)}
    <section class="fx">
      <h3>{g.effect.toUpperCase()}</h3>
      {#if g.description}<p class="desc">{g.description}</p>{/if}
      {#each g.items as it (it.item_pageid)}
        {@const inEra = eraAllowed(it.era, s.build.enabled_eras)}
        <div class="item" class:outofera={!inEra}>
          <span class="bullet">•</span>
          <button class="iname" onclick={() => openUrl(wikiUrl(it.item_name))} title="open on eqlwiki.com">
            {it.item_name}
          </button>
          <span class="classes">[{classChip(it.item_classes)}]</span>
          {#if !inEra}<span class="era">{it.era} — not in era</span>{/if}
        </div>
        {#if it.sources.length > 0}
          {#each foldSources(it.sources) as line, i (i)}
            <div class="src">{line}</div>
          {/each}
        {:else}
          <div class="src dim">quest / unknown source — see the wiki page</div>
        {/if}
      {/each}
    </section>
  {:else}
    <p class="muted">no focus effects at this tier in the data</p>
  {/each}
{/if}

<style>
  .tierrow { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin-bottom: .8rem; }
  .tbtn {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 4px 14px; cursor: pointer; font-size: .82rem; font-variant: small-caps;
    letter-spacing: .05em;
  }
  .tbtn:hover { color: #e6e6e6; }
  .tbtn.on { color: #c9b26a; border-color: #8a7440; background: #241f14; }
  .hint { color: #667; font-size: .72rem; margin-left: auto; }
  .muted { color: #667; }
  .banner {
    color: #c9b26a; font-variant: small-caps; letter-spacing: .18em; font-size: 1rem;
    border-bottom: 1px solid #3a3f4a; padding-bottom: .3rem; margin: 0 0 .8rem;
  }
  .fx { margin-bottom: 1.1rem; max-width: 760px; }
  .fx h3 { color: #d9c67a; letter-spacing: .08em; font-size: .88rem; margin: 0 0 .1rem; }
  .desc { color: #9ab; font-size: .78rem; margin: 0 0 .4rem; }
  .item { display: flex; gap: .45rem; align-items: baseline; padding-top: .25rem; }
  .item.outofera { opacity: .55; }
  .bullet { color: #667; }
  .iname {
    background: none; border: none; padding: 0; cursor: pointer; font: inherit;
    color: #6ac; font-size: .84rem;
  }
  .iname:hover { color: #9cf; text-decoration: underline; }
  .classes { color: #c9b26a; font-size: .72rem; }
  .era { color: #a7c; font-size: .7rem; font-style: italic; }
  .src { color: #89a; font-size: .76rem; margin-left: 1.15rem; }
  .src.dim { color: #667; font-style: italic; }
</style>
