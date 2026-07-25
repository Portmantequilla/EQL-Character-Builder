<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import type { AppState } from "../state.svelte";
  import type { EquipWarning, Item, InventoryImport, InventoryScan } from "../api";
  import { importInventory, listInventoryFiles, optimizeGear, setEqlDir } from "../api";
  import ItemEditPopup from "../ItemEditPopup.svelte";
  import ItemPicker from "../ItemPicker.svelte";
  import ItemTooltip from "../ItemTooltip.svelte";
  import SlotWell from "../SlotWell.svelte";
  import { canonicalSlot, eraAllowed, itemScore, PAPERDOLL_ROWS } from "../format";

  let { s }: { s: AppState } = $props();

  let openSlot = $state<string | null>(null);
  let editSlot = $state<string | null>(null); // slot whose item-edit popup is open

  // a wholesale build swap (File > New/Open/Import, Choose for me — native menu events
  // pierce the modal backdrop) must not leave a latched editSlot that pops the panel
  // back open when a later build fills that slot
  let buildRef: unknown = null;
  $effect(() => {
    const b = s.build; // track the reference only
    if (buildRef !== null && buildRef !== b) {
      editSlot = null;
      openSlot = null;
    }
    buildRef = b;
  });

  // the host item for the edit popup (from the browser list, or a minimal shell when
  // the equipped item is outside the class-filtered list — names/icons via the result)
  const editItem = $derived.by(() => {
    if (editSlot == null) return null;
    const pid = s.build.equipment[editSlot];
    if (pid == null) return null;
    return (
      s.itemsById.get(pid) ?? {
        pageid: pid,
        name: s.result?.equipped_item_names[pid] ?? `item #${pid}`,
        icon_id: s.result?.equipped_item_icons[pid] ?? null,
        slot: null, slots: [], classes: [], races: [], deities: [],
        ac: null, dmg: null, atk_delay: null, weapon_skill: null, haste_pct: null,
        required_level: null, recommended_level: null, stats: {},
        worn_effect: null, focus_effect: null, click_effect: null, era: null,
        flags: null, weight: null, size: null, merchant_value: null,
      }
    );
  });
  // count of socketed augments per slot (left-rail badge + paperdoll hint)
  const augCount = $derived.by(() => {
    const m = new Map<string, number>();
    for (const [slot, socks] of Object.entries(s.build.augments ?? {})) {
      m.set(slot, Object.keys(socks).length);
    }
    return m;
  });

  // ---- import worn gear from a /outputfile inventory dump ----
  let showImport = $state(false);
  let scan = $state<InventoryScan | null>(null); // auto-found dumps in the EQL folder
  let lastImport = $state<InventoryImport | null>(null);
  let importBusy = $state(false);
  let importErr = $state<string | null>(null);

  async function toggleImport() {
    showImport = !showImport;
    if (showImport && scan === null) await rescan();
  }
  async function rescan() {
    try { scan = await listInventoryFiles(); } catch (e) { importErr = String(e); }
  }

  async function doImport(path: string) {
    importBusy = true; importErr = null;
    try {
      const imp = await importInventory(path);
      applyImport(imp);
      lastImport = imp;
      await rescan(); // the EQL folder is now remembered — refresh the quick-pick list
    } catch (e) {
      importErr = String(e);
    } finally {
      importBusy = false;
    }
  }

  async function browseFile() {
    try {
      const path = await open({
        filters: [{ name: "Inventory dump", extensions: ["txt"] }],
        defaultPath: scan?.dir ?? undefined,
      });
      if (typeof path === "string") await doImport(path);
    } catch (e) { importErr = String(e); }
  }

  async function pickFolder() {
    try {
      const dir = await open({ directory: true, defaultPath: scan?.dir ?? undefined });
      if (typeof dir === "string") scan = await setEqlDir(dir);
    } catch (e) { importErr = String(e); }
  }

  /** Mirror the character: replace the worn player slots (keep pet gear) with the import. */
  function applyImport(imp: InventoryImport) {
    const eq: Record<string, number> = { ...s.build.equipment };
    const tiers: Record<string, number> = { ...(s.build.equipment_tiers ?? {}) };
    const augs: Record<string, Record<string, number>> = { ...(s.build.augments ?? {}) };
    for (const k of Object.keys(eq)) if (!k.startsWith("PET_")) delete eq[k];
    for (const k of Object.keys(tiers)) if (!k.startsWith("PET_")) delete tiers[k];
    for (const k of Object.keys(augs)) if (!k.startsWith("PET_")) delete augs[k];
    for (const [slot, pid] of Object.entries(imp.equipment)) eq[slot] = pid;
    for (const [slot, t] of Object.entries(imp.equipment_tiers)) tiers[slot] = t;
    for (const [slot, socks] of Object.entries(imp.augments)) augs[slot] = { ...socks };
    s.build.equipment = eq;       // fresh objects -> the resolve pipeline re-fires
    s.build.equipment_tiers = tiers;
    s.build.augments = augs;
  }

  // ---- one-click gear optimizer (Optimal survival / Min-Max offense) ----
  let optBusy = $state<"OPTIMAL" | "MINMAX" | null>(null);
  let optConfirm = $state<"OPTIMAL" | "MINMAX" | null>(null);
  let optConfirmTimer: ReturnType<typeof setTimeout> | undefined;
  let optSummary = $state<string | null>(null);

  const hasWornGear = $derived(
    Object.keys(s.build.equipment ?? {}).some((k) => !k.startsWith("PET_"))
  );

  function requestOptimize(profile: "OPTIMAL" | "MINMAX") {
    // one-click overwrites all worn gear — confirm once if the paperdoll isn't empty
    if (hasWornGear && optConfirm !== profile) {
      optConfirm = profile;
      clearTimeout(optConfirmTimer);
      optConfirmTimer = setTimeout(() => (optConfirm = null), 3500);
      return;
    }
    clearTimeout(optConfirmTimer);
    optConfirm = null;
    void runOptimize(profile);
  }

  // ---- clear all worn gear (pet gear untouched) — same two-click confirm ----
  let clearConfirm = $state(false);
  let clearConfirmTimer: ReturnType<typeof setTimeout>;
  function requestClearAll() {
    if (!clearConfirm) {
      clearConfirm = true;
      clearTimeout(clearConfirmTimer);
      clearConfirmTimer = setTimeout(() => (clearConfirm = false), 3500);
      return;
    }
    clearTimeout(clearConfirmTimer);
    clearConfirm = false;
    for (const slot of Object.keys(s.build.equipment)) {
      if (slot.startsWith("PET_")) continue;
      clear(slot);
    }
    openSlot = null;
    optSummary = null;
  }

  // include the class epic quest weapons in optimizer suggestions (off = drops only;
  // epics stay hand-pickable in the slot pickers either way)
  let allowEpic = $state(false);

  async function runOptimize(profile: "OPTIMAL" | "MINMAX") {
    optBusy = profile;
    optSummary = null;
    try {
      const next = await optimizeGear($state.snapshot(s.build) as typeof s.build, profile, allowEpic);
      // adopt the optimized worn gear/tiers/augments; keep everything else as returned
      s.build = next;
      editSlot = null;
      openSlot = null;
      const worn = Object.keys(next.equipment).filter((k) => !k.startsWith("PET_")).length;
      const augs = Object.values(next.augments ?? {}).reduce(
        (n, m) => n + Object.keys(m).length, 0
      );
      optSummary = `${profile === "OPTIMAL" ? "Optimal" : "Min-Max"}: ${worn} slots geared`
        + (augs > 0 ? `, ${augs} Exaltation${augs === 1 ? "" : "s"} socketed` : "");
    } catch (e) {
      s.error = String(e);
    } finally {
      optBusy = null;
    }
  }

  const slots = $derived(s.staticData?.paperdoll_slots ?? []);
  const warningsBySlot = $derived.by(() => {
    const m = new Map<string, EquipWarning>();
    for (const w of s.result?.equipment_warnings ?? []) m.set(w.slot, w);
    return m;
  });

  // the in-game inventory window: four stone bars of slots (Equipment Layout
  // reference). Rows only render slots the backend actually reports; anything the
  // backend adds that the rows don't know falls into the overflow row.
  const rows = $derived(
    PAPERDOLL_ROWS.map((row) => row.filter((sl) => slots.includes(sl))).filter(
      (row) => row.length > 0
    )
  );
  const extraSlots = $derived(
    slots.filter((sl) => !PAPERDOLL_ROWS.some((row) => row.includes(sl)))
  );

  // candidates for the open slot: items within the enabled expansions, sorted by
  // ac + sum(stats) desc (ItemPicker caps at 200). ANY slots skip the slot filter
  // and offer ALL class-legal items; every other slot filters by canonical slot.
  const openCandidates = $derived.by(() => {
    if (openSlot === null) return [] as Item[];
    const canon = canonicalSlot(openSlot);
    return s.items
      .filter(
        (i) =>
          canon === "ANY" ||
          i.slots.some((sl) => sl.toUpperCase() === canon) ||
          (i.slot ?? "").toUpperCase() === canon
      )
      .filter((i) => eraAllowed(i.era, s.build.enabled_eras))
      .sort((a, b) => itemScore(b) - itemScore(a));
  });

  function equip(slot: string, item: Item) {
    // a different item must not inherit the previous item's upgrade tier or its
    // socketed augments (they live IN the item) — mirrors clear()
    if (s.build.equipment[slot] !== item.pageid) {
      setTier(slot, 0);
      clearAugments(slot);
    }
    s.build.equipment[slot] = item.pageid;
    openSlot = null;
  }
  function clear(slot: string) {
    delete s.build.equipment[slot];
    // drop the upgrade tier + augments with the item so a future pick doesn't inherit them
    if ((s.build.equipment_tiers ?? {})[slot] != null) setTier(slot, 0);
    clearAugments(slot);
    if (editSlot === slot) editSlot = null;
  }
  function clearAugments(slot: string) {
    if ((s.build.augments ?? {})[slot] == null) return;
    const all = { ...(s.build.augments ?? {}) };
    delete all[slot];
    s.build.augments = all;
  }

  // ---- item upgrade tiers (+0..+10; slot key -> tier, shared map with PET_N) ----
  function tierOf(slot: string): number {
    return s.build.equipment_tiers?.[slot] ?? 0;
  }
  function setTier(slot: string, n: number) {
    const clamped = Math.min(10, Math.max(0, n));
    const next = { ...(s.build.equipment_tiers ?? {}) };
    if (clamped === 0) delete next[slot];
    else next[slot] = clamped;
    s.build.equipment_tiers = next; // fresh object -> resolve pipeline re-fires
  }

  // ---- bulk tier: set every worn player slot to one tier (0..10). The slider tracks
  // its value live but only COMMITS on release (change), so dragging doesn't spam the
  // resolve pipeline. Pet gear (PET_) is managed on the Pet tab and left untouched.
  let bulkTier = $state(0);
  function applyBulkTier(n: number) {
    const clamped = Math.min(10, Math.max(0, Math.round(n)));
    bulkTier = clamped;
    const next: Record<string, number> = { ...(s.build.equipment_tiers ?? {}) };
    // keep pet tiers; retier every equipped worn player slot
    for (const slot of Object.keys(s.build.equipment)) {
      if (slot.startsWith("PET_")) continue;
      if (clamped === 0) delete next[slot];
      else next[slot] = clamped;
    }
    s.build.equipment_tiers = next; // fresh object -> resolve pipeline re-fires
  }
  function glowFor(warn: EquipWarning | undefined): "none" | "red" | "yellow" {
    if (!warn) return "none";
    return warn.status === "SAVED_INACTIVE" ? "red" : "yellow";
  }
  function nameFor(pid: number | undefined, it: Item | undefined, slot: string): string {
    if (it) return it.name;
    if (pid != null) return s.result?.equipped_item_names[pid] ?? `item #${pid}`;
    return slot;
  }
  function stat(name: string): number | null {
    const st = s.result?.stats;
    if (!st) return null;
    const k = Object.keys(st).find((k) => k.toUpperCase().replace(/_/g, " ") === name);
    return k !== undefined ? st[k].capped_total : null;
  }
</script>

{#snippet wellFor(slot: string)}
  {@const pid = s.build.equipment[slot]}
  {@const it = pid != null ? s.itemsById.get(pid) : undefined}
  {@const warn = warningsBySlot.get(slot)}
  {@const tier = pid != null ? tierOf(slot) : 0}
  <SlotWell
    iconId={it?.icon_id ?? (pid != null ? (s.result?.equipped_item_icons[pid] ?? null) : null)}
    label={canonicalSlot(slot)}
    filled={pid != null}
    glow={glowFor(warn)}
    selected={openSlot === slot}
    {tier}
    onclick={() => {
      // filled slot -> the persistent item-details panel; empty -> the picker
      if (pid != null) {
        editSlot = editSlot === slot ? null : slot;
        openSlot = slot; // keep the left rail + tier context on this slot
      } else {
        editSlot = null;
        openSlot = openSlot === slot ? null : slot;
      }
    }}
    onclear={pid != null ? () => clear(slot) : undefined}
  >
    {#snippet tooltip()}
      {#if warn}
        <div class="tipwarn" class:saved={warn.status === "SAVED_INACTIVE"}>
          {warn.reason} — {warn.status}
        </div>
      {/if}
      {#if it}
        <ItemTooltip {s} item={it} {tier} slotKey={slot} />
      {:else if pid != null}
        <div class="tipname">
          {nameFor(pid, it, slot)}{#if tier > 0}<span class="tiptier"> +{tier}</span>{/if}
        </div>
        <div class="tiprow">full details unavailable (outside the class-filtered list)</div>
      {:else}
        <div class="tipname">{slot}</div>
        <div class="tipempty">empty — click to pick</div>
      {/if}
    {/snippet}
  </SlotWell>
{/snippet}

{#if slots.length === 0}
  <p class="muted">Loading paperdoll slots…</p>
{:else}
  <div class="importbar">
    <button class="impbtn" onclick={toggleImport} aria-expanded={showImport}>
      📥 Import from game
    </button>
    {#if lastImport}
      <span class="impstatus">
        {#if lastImport.character}<strong>{lastImport.character}</strong> · {/if}
        {lastImport.matched.length} equipped{#if lastImport.unmatched.length}, <span class="warn">{lastImport.unmatched.length} not found</span>{/if}
      </span>
    {/if}
  </div>

  <div class="optbar">
    <span class="optlbl">Auto-gear:</span>
    <button
      class="optbtn optimal"
      class:confirm={optConfirm === "OPTIMAL"}
      disabled={optBusy != null || s.build.classes.length === 0}
      title="Best gear + Exaltations for survival and longevity — AC, HP, stamina, resists, and an even spread across all attributes"
      onclick={() => requestOptimize("OPTIMAL")}
    >
      {#if optBusy === "OPTIMAL"}optimizing…{:else if optConfirm === "OPTIMAL"}replace all gear?{:else}🛡 Optimal (survival){/if}
    </button>
    <button
      class="optbtn minmax"
      class:confirm={optConfirm === "MINMAX"}
      disabled={optBusy != null || s.build.classes.length === 0}
      title="Max damage / max stats — weapon ratio, ATK, haste, and your offensive stats, with little regard for staying alive"
      onclick={() => requestOptimize("MINMAX")}
    >
      {#if optBusy === "MINMAX"}optimizing…{:else if optConfirm === "MINMAX"}replace all gear?{:else}⚔ Min-Max (damage){/if}
    </button>
    <label
      class="epictgl"
      title="Include the class epic quest weapons in suggestions. Off = the optimizer only suggests obtainable drops; epics stay hand-pickable in the slot pickers either way."
    >
      <input type="checkbox" bind:checked={allowEpic} disabled={optBusy != null} />
      Allow epic gear
    </label>
    <button
      class="optbtn clearall"
      class:confirm={clearConfirm}
      disabled={optBusy != null || !hasWornGear}
      title="Remove every worn item (with its tiers and Exaltations). Pet gear is untouched."
      onclick={requestClearAll}
    >
      {#if clearConfirm}remove all worn gear?{:else}✖ Clear all{/if}
    </button>
    {#if optSummary}<span class="optsummary">{optSummary}</span>{/if}
    <span class="opthint">replaces worn gear (keeps pet, spells, buffs); tiers stay 0 — set them yourself</span>
  </div>

  <div class="tierbar">
    <span class="tierlabel">Set all gear tiers</span>
    <input
      class="tierslider"
      type="range" min="0" max="10" step="1"
      value={bulkTier}
      oninput={(e) => (bulkTier = +e.currentTarget.value)}
      onchange={(e) => applyBulkTier(+e.currentTarget.value)}
      aria-label="set every worn item to this upgrade tier"
    />
    <strong class="tierval">+{bulkTier}</strong>
    <span class="tierhint2">applies to every worn item on release · game-exact upgrade rule; delay never scales</span>
  </div>

  {#if showImport}
    <div class="importpanel">
      <div class="imphead">
        <strong>Import your worn equipment from the game</strong>
        <button class="mini" onclick={() => (showImport = false)}>Close</button>
      </div>
      <ol class="howto">
        <li>In game, click the chat bar, type <code>/outputfile inventory</code>, and press Enter.</li>
        <li>The game saves <code>&lt;Character&gt;_&lt;city&gt;-Inventory.txt</code> into your EQL install folder.</li>
        <li>Pick that file below (or <em>Browse</em> to it). Your worn gear and its <code>+N</code>
          upgrade tiers import automatically — this replaces the worn slots on the paperdoll.</li>
      </ol>

      {#if scan?.dir}
        <div class="scandir">
          EQL folder: <code>{scan.dir}</code>
        </div>
      {/if}

      {#if scan && scan.files.length > 0}
        <div class="filelist">
          {#each scan.files as f (f.path)}
            <button class="filebtn" onclick={() => doImport(f.path)} disabled={importBusy}>
              <span class="fchar">{f.character ?? f.name}</span>
              <span class="fname">{f.name}</span>
            </button>
          {/each}
        </div>
      {:else if scan}
        <p class="none">
          No <code>-Inventory.txt</code> found automatically.
          Use <em>Browse</em> below — or, if EQL is on another drive, set your game folder first.
        </p>
      {/if}

      <div class="improw">
        <button class="impbtn" onclick={browseFile} disabled={importBusy}>Browse for file…</button>
        <button class="mini" onclick={pickFolder} disabled={importBusy}>Set EQL folder…</button>
        {#if scan?.dir}<button class="mini" onclick={rescan} disabled={importBusy}>Re-scan</button>{/if}
        {#if importBusy}<span class="none">importing…</span>{/if}
      </div>

      {#if importErr}<p class="imperr">{importErr}</p>{/if}

      {#if lastImport}
        <div class="impsum">
          <div class="sumhead">
            Imported {lastImport.matched.length} worn item{lastImport.matched.length === 1 ? "" : "s"}{#if lastImport.character} for <strong>{lastImport.character}</strong>{/if}.
          </div>
          {#if lastImport.unmatched.length > 0}
            <div class="sumsub warn">Not in the wiki data — left off the paperdoll so nothing is faked:</div>
            <ul class="sumlist">
              {#each lastImport.unmatched as u, i (i)}
                <li>{u.slot || "—"}: {u.game_name} <span class="dim">({u.reason})</span></li>
              {/each}
            </ul>
          {/if}
          {#if lastImport.exaltations.length > 0}
            <div class="sumsub">
              {lastImport.exaltations.length} Exaltation augment{lastImport.exaltations.length === 1 ? "" : "s"} detected —
              shown for reference; augments don't feed stats yet:
            </div>
            <ul class="sumlist">
              {#each lastImport.exaltations as x, i (i)}
                <li>{x.slot} · socket {x.socket}: {x.name}</li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <div class="eqwrap">
    <div class="leftrail">
      <button
        class="editbtn"
        disabled={openSlot == null || s.build.equipment[openSlot] == null}
        title={openSlot == null
          ? "select a filled slot first"
          : s.build.equipment[openSlot] == null
            ? "that slot is empty"
            : `edit ${openSlot}`}
        onclick={() => (editSlot = openSlot)}
      >✦ Edit item</button>
      {#if openSlot != null && (augCount.get(openSlot) ?? 0) > 0}
        <span class="augnote">{augCount.get(openSlot)} augment{(augCount.get(openSlot) ?? 0) === 1 ? "" : "s"}</span>
      {/if}
      <span class="railhint">augments &amp; Exaltation sockets</span>
    </div>

  <div class="eqpanel">
    <div class="titlebar">Equipment</div>

    <div class="charcard">
      <div class="cid">
        <span class="cname">{s.build.name || "unnamed"}</span>
        <span class="cline">{s.result?.race ?? s.build.race ?? "any race"} · level {s.build.level} · {s.build.classes.join(" / ") || "no classes"}</span>
      </div>
      <div class="cstats">
        <span>AC <strong>{stat("AC") ?? "—"}</strong></span>
        <span>HP <strong>{stat("HP") ?? "—"}</strong></span>
        <span>ATK <strong>{stat("ATK") ?? "—"}</strong></span>
        <span class="chaste">haste +{s.result?.equipment_haste_pct ?? 0}%</span>
      </div>
    </div>

    <div class="rows">
      {#each rows as row, ri (ri)}
        <div class="slotbar">
          {#each row as slot (slot)}
            <div class="barcell">
              {@render wellFor(slot)}
            </div>
          {/each}
        </div>
      {/each}
      {#if extraSlots.length > 0}
        <div class="slotbar">
          {#each extraSlots as slot (slot)}
            <div class="barcell">
              {@render wellFor(slot)}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
  </div>

  {#if editSlot != null && editItem != null}
    <ItemEditPopup
      {s}
      slotKey={editSlot}
      item={editItem}
      tier={tierOf(editSlot)}
      onclose={() => (editSlot = null)}
      onchangeitem={() => { openSlot = editSlot; editSlot = null; }}
    />
  {/if}

  {#if openSlot !== null}
    {@const slot = openSlot}
    <div class="pickerbox">
      <div class="pickhead">
        <span>Pick for <strong>{slot}</strong></span>
        <button class="mini" onclick={() => (openSlot = null)}>Close</button>
      </div>
      {#if s.build.equipment[slot] != null}
        <div class="tierrow">
          <span class="tierlbl">Upgrade tier</span>
          <button class="mini" onclick={() => setTier(slot, tierOf(slot) - 1)} disabled={tierOf(slot) <= 0}>−</button>
          <strong class="tiernum">+{tierOf(slot)}</strong>
          <button class="mini" onclick={() => setTier(slot, tierOf(slot) + 1)} disabled={tierOf(slot) >= 10}>+</button>
          <span class="tierhint">≤10: +1/tier · >10 & dmg: +10%/tier (game-exact rounding) · haste +1%/tier · delay never changes</span>
        </div>
      {/if}
      {#if openCandidates.length === 0}
        <p class="none">no slot data yet (run data import)</p>
      {:else}
        <ItemPicker
          items={openCandidates}
          level={s.build.level}
          onpick={(i) => equip(slot, i)}
          placeholder={`search ${canonicalSlot(slot)} items…`}
        />
      {/if}
    </div>
  {/if}
{/if}

<style>
  .muted { color: #667; }

  /* ---- import-from-game bar + panel ---- */
  .importbar {
    max-width: 760px; margin: 0 auto .6rem; display: flex; align-items: center;
    gap: .8rem; flex-wrap: wrap;
  }
  .impbtn {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 5px 12px; cursor: pointer; font-size: .82rem; font-variant: small-caps;
    letter-spacing: .04em;
  }
  .impbtn:hover:not(:disabled) { border-color: #c9b26a; color: #e6d08a; }
  .impbtn:disabled { opacity: .4; cursor: default; }
  .impstatus { color: #9ab; font-size: .78rem; }
  .impstatus strong { color: #c9b26a; }
  .warn { color: #da5; }

  /* ---- one-click gear optimizer bar ---- */
  .optbar {
    max-width: 760px; margin: 0 auto .6rem; display: flex; align-items: center;
    gap: .5rem; flex-wrap: wrap;
  }
  .optlbl { color: #89a; font-size: .78rem; font-variant: small-caps; letter-spacing: .06em; }
  .optbtn {
    border-radius: 6px; padding: 5px 12px; cursor: pointer; font-size: .82rem;
    font-variant: small-caps; letter-spacing: .04em; border: 1px solid;
  }
  .optbtn:disabled { opacity: .4; cursor: default; }
  .optbtn.optimal { background: #14201a; color: #7c9; border-color: #2a6; }
  .optbtn.optimal:hover:not(:disabled) { background: #172a20; color: #9ec; }
  .optbtn.minmax { background: #211618; color: #d88; border-color: #a44; }
  .optbtn.minmax:hover:not(:disabled) { background: #2a181a; color: #f9a; }
  .optbtn.confirm { background: #3a2e12; color: #fc6; border-color: #c9b26a; }
  .optbtn.clearall { background: #1c1c22; color: #99a; border-color: #445; }
  .optbtn.clearall:hover:not(:disabled) { background: #26262e; color: #bbc; }
  .epictgl { display: inline-flex; align-items: center; gap: 5px; color: #8a7440;
    font-size: .72rem; cursor: pointer; user-select: none; margin-left: .3rem; }
  .epictgl input { accent-color: #8a7440; cursor: pointer; }
  .optsummary { color: #6c9; font-size: .78rem; }
  .opthint { color: #667; font-size: .68rem; margin-left: auto; }

  /* ---- bulk gear-tier slider ---- */
  .tierbar {
    max-width: 760px; margin: 0 auto .6rem; display: flex; align-items: center;
    gap: .6rem; flex-wrap: wrap;
  }
  .tierlabel { color: #c9b26a; font-size: .78rem; font-variant: small-caps; letter-spacing: .06em; }
  .tierslider { width: 180px; accent-color: #c9b26a; cursor: pointer; }
  .tierval { color: #c9b26a; min-width: 2rem; text-align: center; }
  .tierhint2 { color: #667; font-size: .68rem; margin-left: auto; }

  .importpanel {
    max-width: 760px; margin: 0 auto .7rem; padding: .7rem .9rem;
    background: #12151c; border: 1px solid #3a3f4a; border-radius: 8px;
  }
  .imphead {
    display: flex; justify-content: space-between; align-items: center; margin-bottom: .5rem;
  }
  .imphead strong { color: #c9b26a; font-size: .9rem; }
  .howto { margin: 0 0 .6rem; padding-left: 1.2rem; color: #9ab; font-size: .8rem; line-height: 1.5; }
  .howto code {
    background: #0c0e13; border: 1px solid #2a2f38; border-radius: 4px;
    padding: 0 4px; color: #cbd; font-size: .78rem;
  }
  .scandir { color: #778; font-size: .74rem; margin-bottom: .5rem; word-break: break-all; }
  .scandir code { color: #9ab; }

  .filelist { display: flex; flex-direction: column; gap: 5px; margin-bottom: .55rem; }
  .filebtn {
    display: flex; align-items: baseline; gap: .6rem; text-align: left;
    background: #171a20; color: #cbd; border: 1px solid #2a2f38; border-radius: 6px;
    padding: 6px 10px; cursor: pointer;
  }
  .filebtn:hover:not(:disabled) { border-color: #2a6; background: #191d24; }
  .filebtn:disabled { opacity: .5; cursor: default; }
  .fchar { color: #c9b26a; font-weight: 600; font-size: .84rem; }
  .fname { color: #778; font-size: .72rem; }

  .improw { display: flex; align-items: center; gap: .6rem; flex-wrap: wrap; }
  .imperr { color: #f66; font-size: .78rem; margin: .5rem 0 0; }

  .impsum { margin-top: .7rem; border-top: 1px solid #262b33; padding-top: .55rem; }
  .sumhead { color: #6c9; font-size: .82rem; }
  .sumhead strong { color: #c9b26a; }
  .sumsub { margin: .45rem 0 .15rem; font-size: .76rem; color: #9ab; }
  .sumsub.warn { color: #da5; }
  .sumlist { margin: 0; padding-left: 1.2rem; color: #9ab; font-size: .76rem; line-height: 1.45; }
  .sumlist .dim { color: #667; }

  /* ---- left rail (edit item / augments) hugging the stone panel ---- */
  .eqwrap {
    display: flex; gap: .6rem; justify-content: center; align-items: flex-start;
  }
  .leftrail {
    display: flex; flex-direction: column; gap: .4rem; width: 96px; flex-shrink: 0;
    position: sticky; top: .5rem;
  }
  .editbtn {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 6px 8px; cursor: pointer; font-size: .78rem; font-variant: small-caps;
    letter-spacing: .04em;
  }
  .editbtn:hover:not(:disabled) { border-color: #c9b26a; color: #e6d08a; }
  .editbtn:disabled { opacity: .38; cursor: default; }
  .augnote { color: #d4f; font-size: .7rem; text-align: center; }
  .railhint { color: #556; font-size: .64rem; text-align: center; line-height: 1.3; }

  /* ---- classic EQ stone panel ---- */
  .eqpanel {
    max-width: 760px; margin: 0 auto;
    background: linear-gradient(160deg, #0d0f14, #1a1d24);
    border: 2px solid #8a7440;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px #3a3f4a, inset 0 0 28px rgba(0, 0, 0, .55);
  }
  .titlebar {
    text-align: center; font-variant: small-caps; letter-spacing: .2em;
    color: #c9b26a; font-size: .8rem; padding: 4px 0;
    background: linear-gradient(#181b22, #12141a);
    border-bottom: 1px solid #3a3f4a;
  }
  /* character summary strip above the slot bars */
  .charcard {
    display: flex; justify-content: space-between; align-items: center; gap: .8rem;
    flex-wrap: wrap; margin: 10px 12px 2px; padding: 7px 12px;
    background: #12151c; border: 1px solid #3a3f4a; border-radius: 3px;
    box-shadow: inset 0 0 14px rgba(0, 0, 0, .6);
  }
  .cid { display: flex; flex-direction: column; gap: 2px; }
  .cname { color: #c9b26a; font-variant: small-caps; letter-spacing: .1em; font-size: .95rem; }
  .cline { color: #9ab; font-size: .74rem; }
  .cstats { display: flex; gap: .9rem; align-items: baseline; color: #89a; font-size: .78rem; }
  .cstats strong { color: #fc6; }
  .chaste { color: #6c9; font-size: .7rem; }

  /* the in-game arrangement: four stone bars of slots (Equipment Layout reference) */
  .rows {
    display: flex; flex-direction: column; gap: 8px; padding: 10px 12px 14px;
    align-items: center;
  }
  .slotbar {
    display: flex; gap: 7px; justify-content: center; flex-wrap: wrap;
    padding: 8px 10px;
    background: linear-gradient(170deg, #14161d, #1c1f27);
    border: 2px ridge #3a3f4a;
    border-radius: 4px;
    box-shadow: inset 0 0 12px rgba(0, 0, 0, .55);
  }
  .barcell { width: 52px; height: 52px; }

  /* ---- upgrade tier stepper (picker panel) ---- */
  .tierrow {
    display: flex; gap: .5rem; align-items: center; margin-bottom: .45rem;
    padding: .3rem .5rem; background: #12151c; border: 1px solid #3a3f4a; border-radius: 6px;
  }
  .tierlbl { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .tiernum { color: #c9b26a; min-width: 2rem; text-align: center; }
  .tierhint { color: #667; font-size: .68rem; margin-left: auto; }

  /* ---- tooltip content (rendered inside SlotWell's tip panel) ---- */
  .tipname { color: #c9b26a; font-weight: 600; margin-bottom: 2px; }
  .tiptier { color: #c9b26a; font-weight: 700; }
  .tipwarn { color: #da5; margin: 2px 0; }
  .tipwarn.saved { color: #f66; }
  .tiprow { color: #9ab; }
  .tipempty { color: #667; font-style: italic; }

  /* ---- picker below the panel ---- */
  .pickerbox {
    max-width: 760px; margin: .7rem auto 0; padding: .5rem;
    background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
  }
  .pickhead {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: .4rem; color: #9ab; font-size: .85rem;
  }
  .pickhead strong { color: #c9b26a; }
  .none { color: #667; font-style: italic; margin: .3rem 0; }
  .mini {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 1px 8px; cursor: pointer; font-size: .75rem;
  }
  .mini:hover:not(:disabled) { color: #e6e6e6; border-color: #2a6; }
  .mini:disabled { opacity: .35; cursor: default; }
</style>
