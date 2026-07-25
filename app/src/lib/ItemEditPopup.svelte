<script lang="ts">
  // The in-game item window, editable: shows the host item like the game's description
  // panel (dark stone, red name, "(Augmented)" title) with the five augment socket rows
  // (Ornamentation + Focus/Click/Worn/Proc Exaltation). Each Exaltation socket opens a
  // searchable dropdown of the augment catalog for that effect kind. Used for BOTH
  // player paperdoll slots and pet ("PET_N") slots.
  import type { AppState } from "./state.svelte";
  import type { AugmentGrant, AugmentInfo, Item } from "./api";
  import { listAugments } from "./api";
  import AugmentCard from "./AugmentCard.svelte";
  import { eraAllowed, hideOnError, iconUrl, itemTierDmg, itemTierStat } from "./format";

  let { s, slotKey, item, tier = 0, onclose, onchangeitem }: {
    s: AppState;
    slotKey: string;          // host slot (paperdoll key or "PET_N")
    item: Item;               // the equipped host item
    tier?: number;            // its upgrade tier (display + stepper)
    onclose: () => void;
    /** close this panel and open the item picker for the slot (swap the item) */
    onchangeitem?: () => void;
  } = $props();

  // tier stepper: PET_N and player slots share build.equipment_tiers
  function setTier(n: number) {
    const clamped = Math.min(10, Math.max(0, n));
    const next = { ...(s.build.equipment_tiers ?? {}) };
    if (clamped === 0) delete next[slotKey];
    else next[slotKey] = clamped;
    s.build.equipment_tiers = next; // fresh object -> resolve pipeline re-fires
  }
  // final cumulative value; up=true marks it green (user spec: show "8", never "3 + 5").
  // Exact community upgrade rule (Mosscovered estimator, Keg-Mallet-confirmed).
  function fin(base: number): { v: number; up: boolean } {
    const v = itemTierStat(base, tier);
    return { v, up: v !== base };
  }
  function finDmg(base: number): { v: number; up: boolean } {
    const v = itemTierDmg(base, tier);
    return { v, up: v !== base };
  }
  const ratio = $derived.by(() => {
    if (item.dmg == null || item.atk_delay == null || item.atk_delay === 0) return null;
    return (finDmg(item.dmg).v / item.atk_delay).toFixed(3);
  });

  // the catalog is static per snapshot — fetch once per app run, share across popups
  let catalog = $state<AugmentInfo[]>([]);
  let catalogErr = $state<string | null>(null);
  $effect(() => {
    getCatalog().then((c) => (catalog = c)).catch((e) => (catalogErr = String(e)));
  });

  // window display order + labels (mirrors eql_data::AUGMENT_SOCKETS / the game window)
  const SOCKETS: { key: string; label: string; effectLabel: string }[] = [
    { key: "ORNAMENTATION", label: "Ornamentation", effectLabel: "" },
    { key: "FOCUS", label: "Focus Exaltation", effectLabel: "Focus Effect" },
    { key: "CLICK", label: "Click Exaltation", effectLabel: "Click Effect" },
    { key: "WORN", label: "Worn Exaltation", effectLabel: "Worn Effect" },
    { key: "PROC", label: "Proc Exaltation", effectLabel: "Combat Effect" },
  ];

  let openSocket = $state<string | null>(null); // which socket's picker is open
  let q = $state("");
  // hovered augment (candidate row or a filled socket) -> the Exaltation card beside
  // the window, like inspecting the augment item in game
  let hoverAug = $state<AugmentInfo | null>(null);
  function catalogEntry(socket: string): AugmentInfo | null {
    const pid = slotAugs[socket];
    if (pid == null) return null;
    return catalog.find((a) => a.source_pageid === pid && a.socket === socket) ?? null;
  }

  const slotAugs = $derived((s.build.augments ?? {})[slotKey] ?? {});
  const grants = $derived(s.result?.augment_grants?.[slotKey] ?? []);
  const augmented = $derived(Object.keys(slotAugs).length > 0);

  function grantFor(socket: string): AugmentGrant | undefined {
    return grants.find((g) => g.socket === socket);
  }
  function catalogName(socket: string): string | null {
    const pid = slotAugs[socket];
    if (pid == null) return null;
    return (
      catalog.find((a) => a.source_pageid === pid && a.socket === socket)?.name ??
      grantFor(socket)?.name ??
      `item #${pid} (Exaltation)`
    );
  }

  // the augment WEARER's classes: pet gear follows the PET's own class pool
  // (user-verified 2026-07-21 — a WAR/BST pet takes WAR-only Exaltations the owner
  // trio couldn't), player gear follows the build trio
  const wearerClasses = $derived(
    slotKey.startsWith("PET_")
      ? (s.result?.pet?.equip_class_pool ?? s.build.classes)
      : s.build.classes
  );

  // candidates for the open socket: the catalog of that effect kind, era-gated like
  // every other picker (out-of-era sources can't exist yet), name-searchable;
  // wearer-class-legal augments first so the sensible picks lead the list
  const candidates = $derived.by(() => {
    if (openSocket == null || openSocket === "ORNAMENTATION") return [] as AugmentInfo[];
    const legal = (a: AugmentInfo) =>
      a.classes.length === 0 ||
      a.classes.includes("ALL") ||
      a.classes.some((c) => wearerClasses.includes(c));
    return catalog
      .filter((a) => a.socket === openSocket)
      .filter((a) => eraAllowed(a.era, s.build.enabled_eras))
      .filter((a) => !q || a.name.toLowerCase().includes(q.toLowerCase())
                       || a.effect_name.toLowerCase().includes(q.toLowerCase()))
      .sort((a, b) => Number(legal(b)) - Number(legal(a)) || a.name.localeCompare(b.name))
      .slice(0, 150);
  });

  const extractTier = $derived(s.staticData?.exaltation_extract_min_tier ?? 4);
  // drag-select guard: only a click that STARTED on the backdrop closes the window
  let downOnBackdrop = false;

  function setAug(socket: string, pid: number | null) {
    const all = { ...(s.build.augments ?? {}) };
    const m = { ...(all[slotKey] ?? {}) };
    if (pid == null) delete m[socket];
    else m[socket] = pid;
    if (Object.keys(m).length === 0) delete all[slotKey];
    else all[slotKey] = m;
    s.build.augments = all; // fresh objects -> resolve pipeline re-fires
    openSocket = null;
    q = "";
    // the clicked row unmounts with the picker — no mouseleave will ever fire
    hoverAug = null;
  }

  function ruleText(a: AugmentInfo): string {
    const parts: string[] = [];
    if (a.classes.length > 0 && !a.classes.includes("ALL")) parts.push(a.classes.join(" "));
    const hands = a.slots.filter((sl) => ["PRIMARY", "SECONDARY", "RANGE"].includes(sl));
    if (hands.length > 0 && hands.length === a.slots.length) parts.push(hands.join("/"));
    return parts.join(" · ");
  }

</script>

<script lang="ts" module>
  import type { AugmentInfo as AI } from "./api";
  import { listAugments as fetchAugments } from "./api";
  // one fetch per app run — the catalog only changes with the wiki snapshot
  let catalogPromise: Promise<AI[]> | null = null;
  function getCatalog(): Promise<AI[]> {
    catalogPromise ??= fetchAugments();
    return catalogPromise;
  }
</script>

<!-- Escape closes from anywhere (window-level: works before focus enters the dialog) -->
<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- backdrop click closes — but only when the press STARTED on the backdrop, so a
     text drag-select that ends outside the window doesn't dismiss it -->
<div
  class="backdrop"
  role="presentation"
  onmousedown={(e) => (downOnBackdrop = e.target === e.currentTarget)}
  onclick={(e) => { if (downOnBackdrop && e.target === e.currentTarget) onclose(); }}
  onkeydown={(e) => e.key === "Escape" && onclose()}
>
  <div class="itemwin" role="dialog" aria-label="Edit item {item.name}" tabindex="-1"
       onclick={(e) => e.stopPropagation()}
       onkeydown={(e) => e.key === "Escape" && onclose()}>
    <div class="titlebar">
      <span>{item.name}{tier > 0 ? ` +${tier}` : ""}{augmented ? " (Augmented)" : ""}</span>
      <button class="x" onclick={onclose} aria-label="close">✕</button>
    </div>
    <div class="subtitle">Description</div>

    <div class="body">
      <div class="head">
        <span class="bigicon">
          {#if item.icon_id != null}
            <img src={iconUrl(item.icon_id)} alt="" draggable="false" onerror={hideOnError} />
          {/if}
        </span>
        <div class="headtext">
          <div class="iname">{item.name}{tier > 0 ? ` +${tier}` : ""}</div>
          {#if item.flags}<div class="irow">{item.flags}</div>{/if}
          {#if item.classes.length > 0}<div class="irow">Class: {item.classes.join(" ")}</div>{/if}
          {#if item.races.length > 0}<div class="irow">Race: {item.races.join(" ")}</div>{/if}
          {#if item.deities.length > 0}<div class="irow">Deity: {item.deities.join(" ")}</div>{/if}
          {#if item.slots.length > 0}<div class="irow">{item.slots.join(" ")}</div>{/if}
          <div class="irow dim">
            {slotKey.startsWith("PET_") ? `Pet slot ${slotKey.slice(4)}` : slotKey}
            {#if item.size != null}&nbsp;·&nbsp;Size: {item.size}{/if}
            {#if item.weight != null}&nbsp;·&nbsp;WT: {item.weight}{/if}
          </div>
        </div>
      </div>

      <div class="tierrow">
        <span class="tierlbl">Upgrade tier</span>
        <button class="mini" onclick={() => setTier(tier - 1)} disabled={tier <= 0}>−</button>
        <strong class="tiernum">+{tier}</strong>
        <button class="mini" onclick={() => setTier(tier + 1)} disabled={tier >= 10}>+</button>
        <span class="tierhint">tooltip and panel show final values · raised numbers in green</span>
      </div>

      <!-- final cumulative values at the current tier (green = raised by upgrades) -->
      <div class="stats">
        {#if item.weapon_skill}<span>Skill: {item.weapon_skill}</span>{/if}
        {#if item.dmg != null}
          {@const d = finDmg(item.dmg)}
          <span>Base Dmg: <span class:up={d.up}>{d.v}</span></span>
        {/if}
        {#if item.atk_delay != null}<span>Delay: {item.atk_delay}</span>{/if}
        {#if ratio != null}<span>Ratio: <span class="ratio">{ratio}</span></span>{/if}
        {#if item.ac != null}
          {@const a = fin(item.ac)}
          <span>AC: <span class:up={a.up}>{a.v}</span></span>
        {/if}
        {#if item.haste_pct != null}<span>Haste: {item.haste_pct}%</span>{/if}
        {#each Object.entries(item.stats) as [k, v] (k)}
          {@const f = v > 0 ? fin(v) : { v, up: false }}
          <span>{k}: <span class:up={f.up}>{f.v}</span></span>
        {/each}
      </div>
      {#if item.required_level != null || item.recommended_level != null}
        <div class="reqrow">
          {#if item.required_level != null}Required level {item.required_level}{/if}
          {#if item.required_level != null && item.recommended_level != null}&nbsp;·&nbsp;{/if}
          {#if item.recommended_level != null}Recommended level {item.recommended_level}{/if}
        </div>
      {/if}

      <div class="sockets">
        {#each SOCKETS as sock (sock.key)}
          {@const name = catalogName(sock.key)}
          {@const grant = grantFor(sock.key)}
          <div class="sockrow" class:open={openSocket === sock.key}>
            <span class="sockbox" class:filled={name != null}></span>
            <span class="socklabel">{sock.label}:</span>
            {#if name != null}
              <button
                class="sockval filled"
                title="change this augment"
                onclick={() => { openSocket = openSocket === sock.key ? null : sock.key; q = ""; }}
                onmouseenter={() => (hoverAug = catalogEntry(sock.key))}
                onmouseleave={() => (hoverAug = null)}
                onfocus={() => (hoverAug = catalogEntry(sock.key))}
                onblur={() => (hoverAug = null)}
              >{name}</button>
              <button class="mini" onclick={() => setAug(sock.key, null)} title="remove augment">✕</button>
            {:else if sock.key === "ORNAMENTATION"}
              <span class="sockval empty" title="cosmetic only — no ornamentation data in the wiki yet">empty</span>
            {:else}
              <button
                class="sockval empty"
                onclick={() => { openSocket = openSocket === sock.key ? null : sock.key; q = ""; }}
              >empty — click to socket</button>
            {/if}
          </div>
          {#if grant != null && grant.warnings.length > 0}
            {#each grant.warnings as w, i (i)}
              <div class="sockwarn">⚠ {w}</div>
            {/each}
          {/if}
        {/each}
      </div>

      <!-- granted effects, worded like the game window -->
      {#each grants.filter((g) => g.effect_name) as g (g.socket)}
        <div class="granted">
          {SOCKETS.find((x) => x.key === g.socket)?.effectLabel ?? "Effect"}:
          <span class="fx">{g.effect_name}</span>
          {#if g.required_level != null}<span class="req">(Req Level {g.required_level})</span>{/if}
        </div>
      {/each}
      {#if item.worn_effect}<div class="granted own">Worn Effect: <span class="fx">{item.worn_effect}</span></div>{/if}
      {#if item.focus_effect}<div class="granted own">Focus Effect: <span class="fx">{item.focus_effect}</span></div>{/if}
      {#if item.click_effect}<div class="granted own">Click Effect: <span class="fx">{item.click_effect}</span></div>{/if}

      {#if item.merchant_value}<div class="valrow">Value: {item.merchant_value}</div>{/if}
      {#if item.era}<div class="erarow">{item.era}</div>{/if}

      {#if onchangeitem}
        <div class="footer">
          <button class="changebtn" onclick={onchangeitem}>⇄ Change item…</button>
        </div>
      {/if}

      {#if openSocket != null && openSocket !== "ORNAMENTATION"}
        <div class="picker">
          <div class="pickhead">
            Socket a <strong>{SOCKETS.find((x) => x.key === openSocket)?.label}</strong>
            <span class="hint">source must be +{extractTier} to extract (rule: VERIFIED_INGAME)</span>
          </div>
          <input placeholder="search augments or effects…" bind:value={q} />
          {#if catalogErr}<p class="err">{catalogErr}</p>{/if}
          <ul>
            {#each candidates as a (a.source_pageid)}
              <li>
                <button
                  class="pick"
                  onclick={() => setAug(openSocket!, a.source_pageid)}
                  onmouseenter={() => (hoverAug = a)}
                  onmouseleave={() => (hoverAug = null)}
                  onfocus={() => (hoverAug = a)}
                  onblur={() => (hoverAug = null)}
                >
                  <span class="ic">
                    {#if a.icon_id != null}
                      {#key a.icon_id}
                        <img src={iconUrl(a.icon_id)} alt="" draggable="false" onerror={hideOnError} />
                      {/key}
                    {/if}
                  </span>
                  <span class="augname">{a.name}</span>
                  <span class="augfx">{a.effect_name}{a.required_level != null ? ` (req ${a.required_level})` : ""}</span>
                  {#if ruleText(a)}<span class="augrule">{ruleText(a)}</span>{/if}
                </button>
              </li>
            {:else}
              <li class="none">{catalog.length === 0 ? "loading catalog…" : "no matches"}</li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  </div>

  {#if hoverAug}
    <div class="augcardbox"><AugmentCard aug={hoverAug} /></div>
  {/if}
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; background: rgba(0, 0, 0, .55); z-index: 60;
    display: flex; align-items: flex-start; justify-content: center; padding-top: 6vh;
  }
  /* the augment inspection card floats beside the window, like a second game window.
     ABSOLUTE + pointer-events:none: mounting it must never shift the centered window
     (that caused a hover flicker loop) or intercept the cursor. */
  .augcardbox {
    position: absolute;
    left: calc(50% + 238px); /* window is 460px centered; 230 + 8px gap */
    top: 14vh;
    pointer-events: none;
  }
  /* narrow windows: no room beside — pin the card to the right edge instead */
  @media (max-width: 800px) {
    .augcardbox { left: auto; right: 4px; top: 10vh; opacity: .96; }
  }
  /* ---- the in-game item description window ---- */
  .itemwin {
    position: relative;
    width: 460px; max-width: 94vw; max-height: 84vh; overflow-y: auto;
    background:
      radial-gradient(ellipse at 30% 10%, #23262e 0%, #14161c 55%, #0d0f13 100%);
    border: 2px solid #8a7440; border-radius: 3px;
    box-shadow: inset 0 0 0 1px #3a3f4a, inset 0 0 40px rgba(0, 0, 0, .6),
                0 8px 40px rgba(0, 0, 0, .7);
    color: #cbd;
  }
  .titlebar {
    display: flex; justify-content: space-between; align-items: center;
    background: linear-gradient(#1d2027, #14161c);
    border-bottom: 1px solid #3a3f4a; padding: 3px 8px;
    color: #d9c67a; font-size: .8rem;
  }
  .x { background: none; border: 1px solid #555; color: #aab; border-radius: 3px;
       cursor: pointer; font-size: .7rem; line-height: 1.1; padding: 0 4px; }
  .x:hover { color: #fff; border-color: #c9b26a; }
  .subtitle {
    text-align: center; color: #e6d76a; font-size: .78rem; padding: 2px 0;
    border-bottom: 1px solid #262b33;
  }
  .body { padding: 10px 14px 14px; }

  .head { display: flex; gap: 10px; margin-bottom: 8px; }
  .bigicon {
    width: 44px; height: 44px; flex-shrink: 0;
    background: #101318; border: 1px solid #2e3440; border-radius: 2px;
    display: flex; align-items: center; justify-content: center;
  }
  .bigicon img { width: 40px; height: 40px; image-rendering: pixelated; }
  .iname { color: #e34; font-weight: 700; }
  .irow { font-size: .78rem; color: #cbd; }
  .irow.dim { color: #778; }

  .stats {
    display: grid; grid-template-columns: 1fr 1fr; gap: 1px 14px;
    font-size: .78rem; color: #cbd; margin-bottom: 10px;
  }
  /* tier-raised finals shown green (final cumulative values, never base+bonus) */
  .up { color: #5d5; font-weight: 600; }
  .ratio { color: #d86; }
  .reqrow { color: #9ab; font-size: .74rem; margin: -6px 0 8px; }
  .valrow { color: #cbd; font-size: .78rem; margin-top: 6px; }
  .erarow { color: #778; font-size: .72rem; font-style: italic; }
  .footer { margin-top: 8px; border-top: 1px solid #262b33; padding-top: 8px; }
  .changebtn {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 4px 12px; cursor: pointer; font-size: .78rem;
  }
  .changebtn:hover { border-color: #c9b26a; color: #e6d08a; }
  .tierrow {
    display: flex; gap: .5rem; align-items: center; margin-bottom: .55rem;
    padding: .3rem .5rem; background: #12151c; border: 1px solid #3a3f4a; border-radius: 6px;
  }
  .tierlbl { color: #c9b26a; font-size: .74rem; font-variant: small-caps; letter-spacing: .08em; }
  .tiernum { color: #c9b26a; min-width: 2rem; text-align: center; }
  .tierhint { color: #667; font-size: .66rem; margin-left: auto; }

  .sockets { border-top: 1px solid #262b33; padding-top: 8px; margin-bottom: 6px; }
  .sockrow { display: flex; align-items: center; gap: 7px; padding: 2px 0; }
  .sockbox {
    width: 14px; height: 14px; flex-shrink: 0;
    background: #101318; border: 1px solid #3a3f4a; border-radius: 2px;
  }
  .sockbox.filled { border-color: #c9b26a; background: #241f10; }
  .socklabel { font-size: .8rem; color: #cbd; }
  .sockval { font-size: .8rem; background: none; border: none; cursor: pointer; font: inherit; }
  .sockval.filled { color: #d4f; }
  .sockval.filled:hover { text-decoration: underline; }
  .sockval.empty { color: #667; font-style: italic; }
  button.sockval.empty:hover { color: #9ab; }
  .sockwarn { color: #da5; font-size: .72rem; margin: 0 0 2px 21px; }
  .mini {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 4px;
    padding: 0 5px; cursor: pointer; font-size: .68rem;
  }
  .mini:hover { color: #f66; border-color: #a44; }

  .granted { font-size: .8rem; margin: 3px 0; color: #cbd; }
  .granted .fx { color: #6ac; }
  .granted.own .fx { color: #6c9; }
  .granted .req { color: #99a; margin-left: 4px; }

  .picker { border-top: 1px solid #262b33; margin-top: 8px; padding-top: 8px; }
  .pickhead { display: flex; justify-content: space-between; align-items: baseline;
              font-size: .78rem; color: #9ab; margin-bottom: 5px; gap: .6rem; }
  .pickhead strong { color: #c9b26a; }
  .hint { color: #667; font-size: .68rem; }
  .picker input {
    width: 100%; box-sizing: border-box; padding: 5px 8px; background: #1c1f26;
    border: 1px solid #333; color: #e6e6e6; border-radius: 6px; margin-bottom: .35rem;
  }
  .picker ul { list-style: none; margin: 0; padding: 0; max-height: 30vh; overflow: auto; }
  .picker li { border-bottom: 1px solid #20242b; }
  .picker li.none { color: #667; font-style: italic; padding: 4px 0; }
  .pick {
    display: flex; gap: .5rem; align-items: center; width: 100%; text-align: left;
    background: none; border: none; color: #e6e6e6; padding: 2px 4px; cursor: pointer;
    font: inherit; font-size: .8rem;
  }
  .pick:hover { background: #22262d; }
  .ic {
    width: 22px; height: 22px; flex-shrink: 0;
    background: #101318; border: 1px solid #2e3440; border-radius: 2px;
    display: flex; align-items: center; justify-content: center;
  }
  .ic img { width: 22px; height: 22px; image-rendering: pixelated; }
  .augname { color: #d4f; flex-shrink: 0; }
  .augfx { color: #6ac; font-size: .74rem; }
  .augrule { margin-left: auto; color: #da5; font-size: .68rem; border: 1px solid #543;
             border-radius: 4px; padding: 0 4px; flex-shrink: 0; }
  .err { color: #f66; font-size: .75rem; }
</style>
