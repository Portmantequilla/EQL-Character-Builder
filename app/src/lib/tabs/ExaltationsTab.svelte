<script lang="ts">
  // EXALTATIONS — informational reference (user request 2026-07-21, formatted after
  // the Focus Effects tab): pick an Exaltation kind (Focus/Click/Worn/Proc), see every
  // effect of that kind with the items that carry it, class restrictions, and where
  // each item drops. Any listed item reaches +4 and its effect becomes
  // "<item> (Exaltation)", socketable into other gear. Item names link to their
  // eqlwiki page. Data: item_effect rows (regen excluded — not extractable) +
  // drops/mobs joins; descriptions come from the effect spells' own wiki pages.
  import type { AppState } from "../state.svelte";
  import { exaltationEffects, openUrl, type ExaltationRow } from "../api";
  import { eraAllowed } from "../format";

  let { s }: { s: AppState } = $props();

  let rows = $state<ExaltationRow[] | null>(null);
  $effect(() => {
    exaltationEffects().then((r) => (rows = r)).catch((e) => (s.error = String(e)));
  });

  const KINDS = [
    { key: "FOCUS", label: "Focus", socket: "Focus Exaltation" },
    { key: "CLICK", label: "Click", socket: "Click Exaltation" },
    { key: "WORN", label: "Worn", socket: "Worn Exaltation" },
    { key: "PROC", label: "Proc", socket: "Proc Exaltation" },
  ] as const;
  type Kind = (typeof KINDS)[number]["key"];
  let kind = $state<Kind>("FOCUS");
  const kindInfo = $derived(KINDS.find((k) => k.key === kind) ?? KINDS[0]);
  // default: only items from the build's enabled expansions; the toggle shows the
  // rest too (dimmed + labeled, same as before)
  let showOutOfEra = $state(false);

  interface Group {
    effect: string;
    description: string | null;
    items: ExaltationRow[];
  }
  const groups = $derived.by(() => {
    const map = new Map<string, Group>();
    for (const r of rows ?? []) {
      if (r.kind !== kind) continue;
      if (!showOutOfEra && !eraAllowed(r.era, s.build.enabled_eras)) continue;
      let g = map.get(r.effect_name);
      if (!g) {
        g = { effect: r.effect_name, description: r.description, items: [] };
        map.set(r.effect_name, g);
      }
      g.description ??= r.description;
      // dedup by pageid — keyed {#each} hard-throws on duplicate keys (Svelte 5 trap)
      if (!g.items.some((x) => x.item_pageid === r.item_pageid)) g.items.push(r);
    }
    return [...map.values()].sort((a, b) => a.effect.localeCompare(b.effect));
  });

  const extractTier = $derived(s.staticData?.exaltation_extract_min_tier ?? 4);

  /** classes chip: the wiki's ALL stays ALL; else the class list joined. */
  function classChip(classes: string[]): string {
    if (classes.length === 0 || classes.includes("ALL")) return "ALL";
    return classes.join(" ");
  }
  /** the Focus tab's source format: mobs sharing a zone fold to "A / B / C - Zone". */
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

<div class="kindrow">
  {#each KINDS as k (k.key)}
    <button class="kbtn" class:on={kind === k.key} onclick={() => (kind = k.key)}>
      {k.label}
    </button>
  {/each}
  <button
    class="erabtn"
    class:on={showOutOfEra}
    onclick={() => (showOutOfEra = !showOutOfEra)}
    title="also list items from expansions outside your enabled set (shown dimmed)"
  >
    {showOutOfEra ? "✓ " : ""}Allow not-in-era
  </button>
  <span class="hint">
    informational — any item below at +{extractTier} extracts its effect as an
    Exaltation · item names open eqlwiki.com
  </span>
</div>

{#if rows == null}
  <p class="muted">loading…</p>
{:else}
  <h2 class="banner">{kindInfo.socket.toUpperCase()}S</h2>
  <p class="explain">
    These items carry a {kindInfo.label.toLowerCase()} effect. Upgrade one to
    +{extractTier} and its effect can be extracted as
    "&lt;item&gt; (Exaltation)" — socketable into another item's
    {kindInfo.socket} slot. The augment keeps the source item's class rules
    (the wearer — player or pet — must match).
  </p>

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
          {#if it.effect_lines.length > 0}
            <span class="fxlines">[{it.effect_lines.join(" · ")}]</span>
          {/if}
          {#if it.required_level != null}
            <span class="req">Req Level {it.required_level}</span>
          {/if}
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
    <p class="muted">no {kindInfo.label.toLowerCase()} effects in the data</p>
  {/each}
{/if}

<style>
  .kindrow { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin-bottom: .8rem; }
  .kbtn {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 4px 14px; cursor: pointer; font-size: .82rem; font-variant: small-caps;
    letter-spacing: .05em;
  }
  .kbtn:hover { color: #e6e6e6; }
  .kbtn.on { color: #c9b26a; border-color: #8a7440; background: #241f14; }
  .erabtn {
    background: #22262d; color: #778; border: 1px solid #333; border-radius: 6px;
    padding: 4px 10px; cursor: pointer; font-size: .74rem; margin-left: .4rem;
  }
  .erabtn:hover { color: #cbd; }
  .erabtn.on { color: #a7c; border-color: #649; background: #1c1424; }
  .hint { color: #667; font-size: .72rem; margin-left: auto; }
  .muted { color: #667; }
  .banner {
    color: #c9b26a; font-variant: small-caps; letter-spacing: .18em; font-size: 1rem;
    border-bottom: 1px solid #3a3f4a; padding-bottom: .3rem; margin: 0 0 .4rem;
  }
  .explain { color: #89a; font-size: .76rem; margin: 0 0 .9rem; max-width: 760px; }
  .fx { margin-bottom: 1.1rem; max-width: 760px; }
  .fx h3 { color: #d9c67a; letter-spacing: .08em; font-size: .88rem; margin: 0 0 .1rem; }
  .desc { color: #9ab; font-size: .78rem; margin: 0 0 .4rem; }
  .item { display: flex; gap: .45rem; align-items: baseline; padding-top: .25rem; flex-wrap: wrap; }
  .item.outofera { opacity: .55; }
  .bullet { color: #667; }
  .iname {
    background: none; border: none; padding: 0; cursor: pointer; font: inherit;
    color: #6ac; font-size: .84rem;
  }
  .iname:hover { color: #9cf; text-decoration: underline; }
  .classes { color: #c9b26a; font-size: .72rem; }
  .fxlines { color: #7c9; font-size: .72rem; }
  .req { color: #f90; font-size: .7rem; }
  .era { color: #a7c; font-size: .7rem; font-style: italic; }
  .src { color: #89a; font-size: .76rem; margin-left: 1.15rem; }
  .src.dim { color: #667; font-style: italic; }
</style>
