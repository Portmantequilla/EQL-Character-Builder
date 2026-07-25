<script lang="ts">
  // The FULL item window content, modeled on the game's description panel (Keg Mallet
  // +5 reference). Shared by the hover tooltips (Equipment + Pet wells) and the
  // persistent item-details panel. Fields that don't exist are omitted, never blank.
  //
  // Upgrade display rule (user spec): show the FINAL cumulative value for the current
  // tier — "Stamina: 8", not "3 + 5" — in green when the tier raised it. Base values
  // stay internal (the engine keeps base + bonus separately).
  import type { AppState } from "./state.svelte";
  import type { Item } from "./api";
  import { itemTierDmg, itemTierHaste, itemTierStat, orderStatKeys } from "./format";

  let { s, item, tier = 0, slotKey }: {
    s: AppState;
    item: Item;
    tier?: number;
    slotKey: string; // host slot (paperdoll key or "PET_N") — for augments/effects
  } = $props();

  // final cumulative value at the item's tier (exact community upgrade rule —
  // Mosscovered estimator, Keg-Mallet-confirmed); changed values render green.
  function fin(base: number): { v: number; up: boolean } {
    const v = itemTierStat(base, tier);
    return { v, up: v !== base };
  }
  function finDmg(base: number): { v: number; up: boolean } {
    const v = itemTierDmg(base, tier);
    return { v, up: v !== base };
  }

  const SOCKET_LABEL: Record<string, string> = {
    ORNAMENTATION: "Ornamentation", FOCUS: "Focus Exaltation", CLICK: "Click Exaltation",
    WORN: "Worn Exaltation", PROC: "Proc Exaltation",
  };
  const SOCKET_ORDER = ["ORNAMENTATION", "FOCUS", "CLICK", "WORN", "PROC"];

  // effects attributed to THIS slot from the engine overview (player slots; carries
  // req levels + augment-granted procs). Pet slots fall back to grants + item strings.
  const fx = $derived(
    (s.result?.effect_overview ?? []).filter((e) => e.source_slot === slotKey)
  );
  const grants = $derived(s.result?.augment_grants?.[slotKey] ?? []);
  const socketed = $derived.by(() => {
    const m = (s.build.augments ?? {})[slotKey] ?? {};
    return SOCKET_ORDER.filter((k) => m[k] != null).map((k) => ({
      socket: k,
      label: SOCKET_LABEL[k],
      grant: grants.find((g) => g.socket === k) ?? null,
    }));
  });
  // own effects: overview rows when present (player), else the item's effect strings
  const ownFx = $derived.by(() => {
    const rows = fx.filter((e) => e.via_augment == null);
    if (rows.length > 0) {
      return rows.map((e) => ({
        label: e.label, name: e.effect_name, req: e.required_level, gated: e.level_gated,
      }));
    }
    const out: { label: string; name: string; req: number | null; gated: boolean }[] = [];
    if (item.worn_effect) out.push({ label: "Worn Effect", name: item.worn_effect, req: null, gated: false });
    if (item.focus_effect) out.push({ label: "Focus Effect", name: item.focus_effect, req: null, gated: false });
    if (item.click_effect) out.push({ label: "Click Effect", name: item.click_effect, req: null, gated: false });
    return out;
  });
  const grantedFx = $derived.by(() => {
    const rows = fx.filter((e) => e.via_augment != null);
    if (rows.length > 0) {
      return rows.map((e) => ({
        label: e.label, name: e.effect_name, req: e.required_level, gated: e.level_gated,
      }));
    }
    // pet slots: the grants themselves
    return grants
      .filter((g) => g.effect_name)
      .map((g) => ({
        label: g.socket === "PROC" ? "Combat Effect"
             : g.socket === "WORN" ? "Worn Effect"
             : g.socket === "CLICK" ? "Click Effect" : "Focus Effect",
        name: g.effect_name, req: g.required_level, gated: false,
      }));
  });

  const ratio = $derived.by(() => {
    if (item.dmg == null || item.atk_delay == null || item.atk_delay === 0) return null;
    return (finDmg(item.dmg).v / item.atk_delay).toFixed(3);
  });
  const statKeys = $derived(orderStatKeys(Object.keys(item.stats)));
</script>

<div class="itemtip">
  <div class="tname">{item.name}{tier > 0 ? ` +${tier}` : ""}{socketed.length > 0 ? " (Augmented)" : ""}</div>
  {#if item.flags}<div class="row flags">{item.flags}</div>{/if}
  {#if item.classes.length > 0}<div class="row">Class: {item.classes.join(" ")}</div>{/if}
  {#if item.races.length > 0}<div class="row">Race: {item.races.join(" ")}</div>{/if}
  {#if item.deities.length > 0}<div class="row">Deity: {item.deities.join(" ")}</div>{/if}
  {#if item.slots.length > 0}
    <div class="row">{item.slots.join(" ")}</div>
  {:else if item.slot}
    <div class="row">{item.slot}</div>
  {/if}

  {#if item.size != null || item.weight != null}
    <div class="row dim">
      {#if item.size != null}Size: {item.size}{/if}
      {#if item.size != null && item.weight != null}&nbsp;·&nbsp;{/if}
      {#if item.weight != null}WT: {item.weight}{/if}
    </div>
  {/if}

  {#if item.weapon_skill || item.dmg != null || item.atk_delay != null}
    <div class="block">
      {#if item.weapon_skill}<div class="row">Skill: {item.weapon_skill}</div>{/if}
      {#if item.dmg != null}
        {@const d = finDmg(item.dmg)}
        <div class="row">Base Dmg: <span class:up={d.up}>{d.v}</span></div>
      {/if}
      {#if item.atk_delay != null}<div class="row">Delay: {item.atk_delay}</div>{/if}
      {#if ratio != null}<div class="row">Ratio: <span class="ratio">{ratio}</span></div>{/if}
    </div>
  {/if}

  {#if item.ac != null || item.haste_pct != null || statKeys.length > 0}
    <div class="block statgrid">
      {#if item.ac != null}
        {@const a = fin(item.ac)}
        <span class="stat">AC: <span class:up={a.up}>{a.v}</span></span>
      {/if}
      {#if item.haste_pct != null}
        {@const h = itemTierHaste(item.haste_pct, tier)}
        <span class="stat">Haste: <span class:up={h !== item.haste_pct}>{h}%</span></span>
      {/if}
      {#each statKeys as k (k)}
        {@const f = fin(item.stats[k])}
        <span class="stat">{k}: <span class:up={f.up}>{f.v}</span></span>
      {/each}
    </div>
  {/if}

  {#if item.required_level != null}<div class="row req">Required level {item.required_level}</div>{/if}
  {#if item.recommended_level != null}<div class="row req">Recommended level {item.recommended_level}</div>{/if}

  {#if socketed.length > 0}
    <div class="block">
      {#each socketed as sk (sk.socket)}
        <div class="row">
          <span class="socklbl">{sk.label}:</span>
          <span class="augname">{sk.grant?.name ?? "socketed"}</span>
        </div>
        {#if sk.grant}
          {#each sk.grant.warnings as w, i (i)}<div class="warn">⚠ {w}</div>{/each}
        {/if}
      {/each}
    </div>
  {/if}

  {#if ownFx.length > 0 || grantedFx.length > 0}
    <div class="block">
      {#each ownFx as e, i ("o" + i)}
        <div class="row fxrow" class:gated={e.gated}>
          {e.label}: <span class="fx">{e.name}</span>
          {#if e.req != null}<span class="freq">(Req Level {e.req})</span>{/if}
          {#if e.gated}<span class="gnote">above your level</span>{/if}
        </div>
      {/each}
      {#each grantedFx as e, i ("g" + i)}
        <div class="row fxrow" class:gated={e.gated}>
          {e.label}: <span class="fx aug">{e.name}</span>
          {#if e.req != null}<span class="freq">(Req Level {e.req})</span>{/if}
          {#if e.gated}<span class="gnote">above your level</span>{/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if item.merchant_value}<div class="row val">Value: {item.merchant_value}</div>{/if}
  {#if item.era}<div class="row era">{item.era}</div>{/if}
</div>

<style>
  .itemtip { font-size: .74rem; color: #cbd; line-height: 1.45; }
  .tname { color: #e34; font-weight: 700; margin-bottom: 2px; }
  .row { color: #cbd; }
  .row.flags { color: #cbd; }
  .row.dim { color: #9ab; }
  .row.req { color: #9ab; }
  .row.val { color: #cbd; margin-top: 3px; }
  .row.era { color: #778; font-style: italic; margin-top: 1px; }
  .block { border-top: 1px solid #262b33; margin-top: 4px; padding-top: 3px; }
  .statgrid { display: grid; grid-template-columns: 1fr 1fr; gap: 0 10px; }
  .stat { white-space: nowrap; }
  /* tier-raised finals: green, per the user's display spec */
  .up { color: #5d5; font-weight: 600; }
  .ratio { color: #d86; }
  .socklbl { color: #cbd; }
  .augname { color: #d4f; }
  .warn { color: #da5; font-size: .68rem; margin-left: 10px; }
  .fx { color: #6ac; }
  .fx.aug { color: #d4f; }
  .freq { color: #99a; margin-left: 3px; }
  .fxrow.gated .fx { color: #567; }
  .gnote { color: #da5; font-size: .66rem; margin-left: 4px; }
</style>
