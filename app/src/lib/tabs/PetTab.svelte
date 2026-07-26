<script lang="ts">
  import type { AppState } from "../state.svelte";
  import type { Item, PetGearSlot, PetSummonInfo } from "../api";
  import { queryItems, optimizePetGear } from "../api";
  import LinesTable from "../LinesTable.svelte";
  import ItemEditPopup from "../ItemEditPopup.svelte";
  import ItemPicker from "../ItemPicker.svelte";
  import ItemTooltip from "../ItemTooltip.svelte";
  import SlotWell from "../SlotWell.svelte";
  import { canonicalSlot, eraAllowed, itemScore, PAPERDOLL_ROWS } from "../format";

  let { s }: { s: AppState } = $props();

  let openPetSlot = $state<string | null>(null);
  let petEditSlot = $state<string | null>(null);    // pet slot whose edit popup is open

  // the pet paperdoll mirrors the player's in-game window rows, PET_-prefixed
  const PET_ROWS = PAPERDOLL_ROWS.map((row) => row.map((sl) => `PET_${sl}`));
  /** "PET_EAR1" -> "EAR" (the well's display label + the picker's slot filter) */
  const petCanon = (petSlot: string) => canonicalSlot(petSlot.replace(/^PET_/, ""));

  // wholesale build swap (native menu New/Open/Import, Choose for me) -> drop latched
  // panel/picker state so the popup can't re-mount uninvited on a later build
  let buildRef: unknown = null;
  $effect(() => {
    const b = s.build; // track the reference only
    if (buildRef !== null && buildRef !== b) {
      petEditSlot = null;
      openPetSlot = null;
    }
    buildRef = b;
  });
  // items usable by the PET's class pool = the pet's OWN classes (user-verified
  // 2026-07-21; owner classes only as fallback when the summon's are unknown). The
  // player browser list (s.items) is filtered to the OWNER's classes and would hide
  // e.g. WAR-only weapons a WAR/BST pet can use, so fetch for the pool itself.
  let petItems = $state<Item[]>([]);
  let petItemsGen = 0;
  $effect(() => {
    const pool = [...(s.result?.pet?.equip_class_pool ?? [])];
    const g = ++petItemsGen;
    if (pool.length === 0) { petItems = []; return; }
    queryItems(pool)
      .then((r) => { if (g === petItemsGen) petItems = r; })
      .catch((e) => { if (g === petItemsGen) s.error = String(e); });
  });
  const petItemsById = $derived(new Map(petItems.map((i) => [i.pageid, i])));

  interface Eligible { ps: PetSummonInfo; cls: string; clsLevel: number; }

  // summons castable by the build: class_levels intersects build classes at <= level
  const eligible = $derived.by(() => {
    const out: Eligible[] = [];
    for (const ps of s.staticData?.pet_summons ?? []) {
      const matches = Object.entries(ps.class_levels)
        .filter(([c, lvl]) => s.build.classes.includes(c) && lvl <= s.build.level)
        .sort((a, b) => a[1] - b[1]);
      if (matches.length > 0) out.push({ ps, cls: matches[0][0], clsLevel: matches[0][1] });
    }
    return out.sort((a, b) => b.clsLevel - a.clsLevel || a.ps.name.localeCompare(b.ps.name));
  });

  const pet = $derived(s.result?.pet ?? null);
  // the saved summon may no longer be castable (class/level change): keep it visible
  // in the select as an explicit stale entry rather than a blank (plan §18)
  const staleSelection = $derived.by(() => {
    const sid = s.build.pet_summon_spell_id;
    if (sid == null || eligible.some((e) => e.ps.spell_id === sid)) return null;
    return s.staticData?.pet_summons.find((p) => p.spell_id === sid) ?? null;
  });

  function setTier(d: number) {
    s.build.pet_summon_tier = Math.min(10, Math.max(0, s.build.pet_summon_tier + d));
  }

  // pet gear picker: ONLY the PET pool list (queryItems for the pet's own classes —
  // user-verified 2026-07-21: a WAR/BST pet uses WAR gear, and the owner's trio does
  // NOT extend the pool). Filtered by the OPEN WELL's canonical slot (ANY wells take
  // everything), era-filtered, limited to real equipment. Hand wells sort dmg-first.
  const petCandidates = $derived.by(() => {
    if (openPetSlot === null) return [] as Item[];
    const canon = petCanon(openPetSlot);
    const handWell = canon === "PRIMARY" || canon === "SECONDARY";
    return [...petItems]
      .filter((i) => eraAllowed(i.era, s.build.enabled_eras))
      .filter((i) => i.dmg != null || i.slots.length > 0 || i.slot != null)
      .filter(
        (i) =>
          canon === "ANY" ||
          i.slots.some((sl) => sl.toUpperCase() === canon) ||
          (i.slot ?? "").toUpperCase() === canon
      )
      .sort((a, b) => {
        if (handWell) {
          const aw = a.dmg != null ? 1 : 0;
          const bw = b.dmg != null ? 1 : 0;
          if (aw !== bw) return bw - aw;
          if (aw === 1 && (a.dmg ?? 0) !== (b.dmg ?? 0)) return (b.dmg ?? 0) - (a.dmg ?? 0);
        }
        return itemScore(b) - itemScore(a);
      });
  });

  // host item for the pet edit popup (pool list, player list, or a minimal shell)
  const petEditItem = $derived.by(() => {
    if (petEditSlot == null) return null;
    const pid = (s.build.pet_equipment ?? {})[petEditSlot];
    if (pid == null) return null;
    return (
      petItemsById.get(pid) ?? s.itemsById.get(pid) ?? {
        pageid: pid,
        name: s.result?.pet?.gear.find((g) => g.slot === petEditSlot)?.item_name ?? `item #${pid}`,
        icon_id: null, slot: null, slots: [], classes: [], races: [], deities: [],
        ac: null, dmg: null, atk_delay: null, weapon_skill: null, haste_pct: null,
        required_level: null, recommended_level: null, stats: {},
        worn_effect: null, focus_effect: null, click_effect: null, era: null,
        flags: null, weight: null, size: null, merchant_value: null,
      }
    );
  });

  // ---- pet inventory slot count: data-derived default, with a manual override the user
  // can set to what they see in game (the derived rule is only PARTIALLY_VERIFIED) ----
  const PET_SLOT_MAX = 12; // mirrors eql_data::PET_SLOT_MAX (bonuses SUM; MAG/BST/NEC = 12)
  function setSlots(n: number) {
    s.build.pet_slot_override = Math.min(PET_SLOT_MAX, Math.max(1, Math.round(n)));
  }
  function resetSlots() {
    s.build.pet_slot_override = null;
  }

  // pet stats to surface on the page (scaled by ACTUAL levels gained — official rule);
  // AC comes from the summed gear
  const petStats = $derived.by(() => {
    const p = s.result?.pet;
    if (!p) return [] as [string, string][];
    const rows: [string, string][] = [];
    if (p.calculated_level != null) rows.push(["Level", `L${p.calculated_level}`]);
    const hp = p.pet_hp_scaled ?? p.summon.pet_hp;
    if (hp != null) rows.push(["HP", String(hp)]);
    const mh = p.pet_max_hit_scaled ?? p.summon.pet_max_hit;
    if (mh != null) rows.push(["Max hit", String(mh)]);
    if (p.skill_point_bonus > 0) rows.push(["Skill pts", `+${p.skill_point_bonus}`]);
    const ac = p.gear_totals?.["AC"];
    if (ac) rows.push(["AC (gear)", `${ac >= 0 ? "+" : ""}${ac}`]);
    return rows;
  });

  // "wearing" = active given items that AREN'T hand items (weapons/shields are shown in
  // the weapon panel from the engine's authoritative weapon_config instead)
  const armorWorn = $derived.by(() => {
    const p = s.result?.pet;
    if (!p) return [] as { petSlot: string; name: string; slot: string }[];
    const wslots = new Set(p.weapon_config.map((w) => w.slot));
    return p.gear
      .filter((g) => g.item_pageid != null && !wslots.has(g.slot)
        && (g.badge === "FULLY_ACTIVE" || g.badge === "PROC_INACTIVE"))
      .map((g) => {
        const it = g.item_pageid != null ? petItemsById.get(g.item_pageid) : undefined;
        const slot = it?.slots.join(", ") || it?.slot || "?";
        // petSlot (PET_N) keys the list — two copies of the same item must not collide
        return { petSlot: g.slot, name: g.item_name ?? "?", slot: slot.toLowerCase() };
      });
  });
  function handLabel(w: { active: boolean; hand: string | null }): string {
    if (!w.active) return "not wielded";
    return w.hand === "PRIMARY" ? "primary" : w.hand === "SECONDARY" ? "secondary" : "wielded";
  }

  // fresh objects so the resolve_build $effect pipeline re-fires
  function equipPet(slotKey: string, item: Item) {
    // a different item must not inherit the previous item's upgrade tier or its
    // socketed augments (they live IN the item) — mirrors clearPet
    if ((s.build.pet_equipment ?? {})[slotKey] !== item.pageid) {
      setGearTier(slotKey, 0);
      clearPetAugments(slotKey);
    }
    s.build.pet_equipment = { ...(s.build.pet_equipment ?? {}), [slotKey]: item.pageid };
    openPetSlot = null;
  }
  function clearPet(slotKey: string) {
    const next = { ...(s.build.pet_equipment ?? {}) };
    delete next[slotKey];
    s.build.pet_equipment = next;
    // drop the upgrade tier + augments with the item so a future pick doesn't inherit them
    if ((s.build.equipment_tiers ?? {})[slotKey] != null) setGearTier(slotKey, 0);
    clearPetAugments(slotKey);
    if (petEditSlot === slotKey) petEditSlot = null;
  }
  function clearPetAugments(slotKey: string) {
    if ((s.build.augments ?? {})[slotKey] == null) return;
    const all = { ...(s.build.augments ?? {}) };
    delete all[slotKey];
    s.build.augments = all;
  }

  // ---- one-click PET gear suggester (Optimal survival / Min-Max offense) ----
  // Fills only the pet's active-slot budget with the best items its class pool can wear.
  let petOptBusy = $state<"OPTIMAL" | "MINMAX" | null>(null);
  let petOptConfirm = $state<"OPTIMAL" | "MINMAX" | null>(null);
  let petOptConfirmTimer: ReturnType<typeof setTimeout> | undefined;
  let petOptSummary = $state<string | null>(null);

  const hasPetGear = $derived(Object.keys(s.build.pet_equipment ?? {}).length > 0);
  
  let clearPetConfirm = $state(false);
  let clearPetConfirmTimer: ReturnType<typeof setTimeout>;
  function requestClearAllPet() {
    if (!clearPetConfirm) {
      clearPetConfirm = true;
      clearTimeout(clearPetConfirmTimer);
      clearPetConfirmTimer = setTimeout(() => (clearPetConfirm = false), 3500);
      return;
    }
    clearTimeout(clearPetConfirmTimer);
    clearPetConfirm = false;
    for (const slot of Object.keys(s.build.pet_equipment ?? {})) {
      clearPet(slot);
    }
    openPetSlot = null;
    petOptSummary = null;
  }

  function requestPetOptimize(profile: "OPTIMAL" | "MINMAX") {
    if (hasPetGear && petOptConfirm !== profile) {
      petOptConfirm = profile;
      clearTimeout(petOptConfirmTimer);
      petOptConfirmTimer = setTimeout(() => (petOptConfirm = null), 3500);
      return;
    }
    clearTimeout(petOptConfirmTimer);
    petOptConfirm = null;
    void runPetOptimize(profile);
  }
  async function runPetOptimize(profile: "OPTIMAL" | "MINMAX") {
    petOptBusy = profile;
    petOptSummary = null;
    try {
      const next = await optimizePetGear($state.snapshot(s.build) as typeof s.build, profile);
      s.build = next;
      petEditSlot = null;
      const geared = Object.keys(next.pet_equipment ?? {}).length;
      petOptSummary = `${profile === "OPTIMAL" ? "Optimal" : "Min-Max"}: ${geared} pet slot${geared === 1 ? "" : "s"} geared`;
    } catch (e) {
      s.error = String(e);
    } finally {
      petOptBusy = null;
    }
  }

  // ---- item upgrade tiers: "PET_N" keys share build.equipment_tiers with player slots ----
  function tierOf(slotKey: string): number {
    return s.build.equipment_tiers?.[slotKey] ?? 0;
  }
  function setGearTier(slotKey: string, n: number) {
    const clamped = Math.min(10, Math.max(0, n));
    const next = { ...(s.build.equipment_tiers ?? {}) };
    if (clamped === 0) delete next[slotKey];
    else next[slotKey] = clamped;
    s.build.equipment_tiers = next; // fresh object -> resolve pipeline re-fires
  }

  // green = active (in the class-combo budget), red = inactive; orange = active but
  // the proc sleeps. Empty wells stay green while the budget has room, red once full.
  function badgeGlow(g: PetGearSlot): "green" | "orange" | "red" {
    if (g.item_pageid == null) return g.active ? "green" : "red";
    switch (g.badge) {
      case "FULLY_ACTIVE": return "green";
      case "PROC_INACTIVE": return "orange";
      default: return "red"; // INVALID_CLASS | OUT_OF_ERA | OVER_CAP
    }
  }
  // the engine's 23 wells keyed by slot, for row-ordered rendering
  const gearBySlot = $derived(new Map((pet?.gear ?? []).map((g) => [g.slot, g])));
  const filledCount = $derived((pet?.gear ?? []).filter((g) => g.item_pageid != null).length);

  const gearTotals = $derived(
    Object.entries(s.result?.pet?.gear_totals ?? {}).filter(([, v]) => v !== 0)
  );
</script>

<div class="controls">
  <label>
    Summon
    <select bind:value={s.build.pet_summon_spell_id}>
      <option value={null}>— no pet —</option>
      {#if staleSelection}
        <option value={staleSelection.spell_id}>
          ⚠ {staleSelection.name} (not castable by this build)
        </option>
      {/if}
      {#each eligible as e (e.ps.spell_id)}
        <option value={e.ps.spell_id}>
          {e.ps.name} ({e.cls} {e.clsLevel}) — pet L{e.ps.base_pet_level ?? "?"}
        </option>
      {/each}
    </select>
  </label>

  <div class="tier">
    <span>Focus tier</span>
    <button onclick={() => setTier(-1)} disabled={s.build.pet_summon_tier <= 0}>−</button>
    <strong>{s.build.pet_summon_tier}</strong>
    <button onclick={() => setTier(1)} disabled={s.build.pet_summon_tier >= 10}>+</button>
  </div>

  <span class="hint">{eligible.length} summons castable at level {s.build.level}</span>
</div>

{#if !pet}
  <p class="muted">No pet resolved — pick a summon above.</p>
{:else}
  <div class="block" class:stale={!pet.valid}>
    <h2>
      {pet.summon.name}
      {#if !pet.valid}
        <span class="unknown">
          — saved but inactive{#if pet.becomes_valid_at}, castable again at level {pet.becomes_valid_at}{/if}
        </span>
      {/if}
    </h2>
    <div class="statrow">
      {#each petStats as [k, v] (k)}
        <span class="stat"><span class="sk">{k}</span> <strong>{v}</strong></span>
      {/each}
      {#if pet.calculated_level == null}<span class="unknown">level unknown — needs in-game test</span>{/if}
      {#if pet.effective_tier > 0}
        <span class="dim">
          tier +{pet.effective_tier}{pet.levels_gained != null ? ` → +${pet.levels_gained} levels` : ""}
        </span>
      {/if}
      {#if pet.tier_capped}
        <span class="capped" title="tier ranks above player level − 1 grant no stats (official rule)">
          capped at player −1
        </span>
      {/if}
      {#if pet.summon.estimate_confidence}
        <span class="estimate" title="level/HP/hit partly from the research workbook — validate in game">
          {pet.summon.estimate_confidence}
        </span>
      {/if}
    </div>

    <div class="chiprow">
      <span class="lbl">intrinsic classes</span>
      {#each pet.intrinsic_classes as c}<span class="chip">{c}</span>{:else}<span class="dim">none</span>{/each}
    </div>
    <div class="chiprow">
      <span class="lbl">equip class pool</span>
      {#each pet.equip_class_pool as c}<span class="chip pool">{c}</span>{:else}<span class="dim">none</span>{/each}
    </div>

    <h3>Pet buff lines ({pet.buff_lines.length})</h3>
    <LinesTable lines={pet.buff_lines} />

    {#if pet.notes.length > 0}
      <h3>Notes</h3>
      <ul class="notes">{#each pet.notes as n}<li>{n}</li>{/each}</ul>
    {/if}
  </div>
{/if}

{#if !pet}
  <p class="muted invhint">Pet inventory — select a summon first.</p>
{:else}
  <div class="invrow">
  <div class="invpanel">
    <div class="titlebar">Pet Inventory</div>
    <div class="slotctl">
      <span class="slotlbl">Active slots</span>
      <input
        type="range" min="1" max={PET_SLOT_MAX} step="1"
        value={pet.slot_count}
        oninput={(e) => setSlots(+e.currentTarget.value)}
        aria-label="pet active slot count"
      />
      <strong class="slotnum">{pet.slot_count}</strong>
      <span class="slothint">
        {#if pet.slot_count_overridden}
          manual override —
          <button class="linkbtn" onclick={resetSlots}>use default ({pet.default_slot_count})</button>
        {:else}
          base 4{pet.slot_bonus_class ? ` + ${pet.slot_bonus_class}` : ""} = {pet.default_slot_count} for this combo · drag to match your game
        {/if}
      </span>
    </div>

    <div class="petopt">
      <span class="petoptlbl">Suggest gear:</span>
      <button
        class="petoptbtn" class:confirm={petOptConfirm === "OPTIMAL"}
        disabled={petOptBusy !== null}
        title="fill the pet's active slots with the best survival gear (AC, HP, stamina, resists) its class can wear"
        onclick={() => requestPetOptimize("OPTIMAL")}
      >
        {#if petOptBusy === "OPTIMAL"}optimizing…{:else if petOptConfirm === "OPTIMAL"}replace pet gear?{:else}🛡 Optimal (survival){/if}
      </button>
      <button
        class="petoptbtn" class:confirm={petOptConfirm === "MINMAX"}
        disabled={petOptBusy !== null}
        title="fill the pet's active slots for maximum offense (weapon ratio, ATK, haste, STR)"
        onclick={() => requestPetOptimize("MINMAX")}
      >
        {#if petOptBusy === "MINMAX"}optimizing…{:else if petOptConfirm === "MINMAX"}replace pet gear?{:else}⚔ Min-Max (damage){/if}
      </button>
      <button
        class="petoptbtn clearall" class:confirm={clearPetConfirm}
        disabled={petOptBusy !== null || !hasPetGear}
        title="Remove every pet gear item (with its tiers and augments). Player gear is untouched."
        onclick={requestClearAllPet}
      >
        {#if clearPetConfirm}remove all pet gear?{:else}✖ Clear all{/if}
      </button>
      {#if petOptSummary}<span class="petoptsum">{petOptSummary}</span>{/if}
    </div>

    {#if filledCount >= pet.slot_count}
      <div class="capnote" class:over={filledCount > pet.slot_count}>
        {#if filledCount > pet.slot_count}
          {filledCount} items given but only {pet.slot_count} slots for this class combo —
          the extras (red) contribute nothing.
        {:else}
          The maximum allowable slots have been filled for this class combo
          ({pet.slot_count}/{pet.slot_count}).
        {/if}
      </div>
    {/if}

    <div class="rows">
      {#each PET_ROWS as row, ri (ri)}
        <div class="slotbar">
          {#each row as slotKey (slotKey)}
            {@const g = gearBySlot.get(slotKey)}
            <div class="barcell">
              <SlotWell
                iconId={g?.icon_id ?? null}
                label={petCanon(slotKey)}
                filled={g?.item_pageid != null}
                glow={g ? badgeGlow(g) : "none"}
                locked={g != null && g.item_pageid == null && !g.active}
                selected={openPetSlot === slotKey}
                tier={g?.item_pageid != null ? tierOf(slotKey) : 0}
                onclick={() => {
                  // filled slot -> the persistent item-details panel; empty -> the picker
                  if (g?.item_pageid != null) {
                    petEditSlot = petEditSlot === slotKey ? null : slotKey;
                    openPetSlot = slotKey;
                  } else {
                    petEditSlot = null;
                    openPetSlot = openPetSlot === slotKey ? null : slotKey;
                  }
                }}
                onclear={g?.item_pageid != null ? () => clearPet(slotKey) : undefined}
              >
                {#snippet tooltip()}
                  {#if g && g.badge !== "EMPTY"}<div class="tipbadge b-{g.badge}">{g.badge}</div>{/if}
                  {#if g?.reason}<div class="tipreason">{g.reason}</div>{/if}
                  {@const it = g?.item_pageid != null
                    ? (petItemsById.get(g.item_pageid) ?? s.itemsById.get(g.item_pageid))
                    : undefined}
                  {#if it && g}
                    <ItemTooltip {s} item={it} tier={tierOf(g.slot)} slotKey={g.slot} />
                  {:else if g?.item_pageid != null}
                    <div class="tipname">{g.item_name ?? `item #${g.item_pageid}`}</div>
                  {:else if g && !g.active}
                    <div class="tipname">{petCanon(slotKey)}</div>
                    <div class="tipempty">
                      no free pet slots — this class combo allows {pet.slot_count}
                    </div>
                  {:else}
                    <div class="tipname">{petCanon(slotKey)}</div>
                    <div class="tipempty">click to pick an item</div>
                  {/if}
                {/snippet}
              </SlotWell>
            </div>
          {/each}
        </div>
      {/each}
    </div>

    {#if pet.weapon_config.length > 0}
      <div class="wornlist">
        <span class="lbl">Weapons</span>
        {#if pet.weapon_summary}<div class="wsum">{pet.weapon_summary}</div>{/if}
        <ul>
          {#each pet.weapon_config as w (w.slot)}
            <li class:inactive={!w.active}>
              <span class="role">{handLabel(w)}</span>
              <span class="wname">{w.item_name}</span>
              <span class="wcat">{w.category}</span>
              {#if w.note}<span class="wproc">{w.note}</span>{/if}
            </li>
          {/each}
        </ul>
        {#each pet.weapon_warnings as warn}<div class="wwarn">⚠ {warn}</div>{/each}
        <div class="wrule">Pets wield one 2H, or two 1H, or one 1H + a shield/off-hand.</div>
      </div>
    {/if}

    {#if armorWorn.length > 0}
      <div class="wornlist">
        <span class="lbl">Wearing (auto-equipped)</span>
        <ul>
          {#each armorWorn as a (a.petSlot)}
            <li><span class="role">{a.slot}</span><span class="wname">{a.name}</span></li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if gearTotals.length > 0}
      <div class="totals">
        <span class="lbl">Pet gear totals</span>
        {#each gearTotals as [k, v] (k)}
          <span class="tchip">{k} {v >= 0 ? "+" : ""}{v}</span>
        {/each}
      </div>
    {/if}

    <p class="legend">
      <span class="lg green">green</span> = active slot ·
      <span class="lg orange">orange</span> = active, proc asleep ·
      <span class="lg red">red</span> = inactive (over the class-combo budget, wrong class, or out of era)
    </p>
  </div>

  {#if openPetSlot !== null}
    {@const slotKey = openPetSlot}
    <div class="pickerbox">
      <div class="pickhead">
        <span>
          Pick for pet <strong>{petCanon(slotKey)}</strong>
          {#if petCanon(slotKey) === "PRIMARY" || petCanon(slotKey) === "SECONDARY"}
            <span class="cathint">wielded by the hand rule (one 2H / two 1H / 1H + shield)</span>
          {:else if petCanon(slotKey) === "ANY"}
            <span class="cathint">anything class-legal, any type</span>
          {/if}
        </span>
        <span class="pickbtns">
          {#if (s.build.pet_equipment ?? {})[slotKey] != null}
            <button class="minibtn edit" onclick={() => (petEditSlot = slotKey)}>✦ Edit item</button>
          {/if}
          <button class="minibtn" onclick={() => (openPetSlot = null)}>Close</button>
        </span>
      </div>
      {#if (s.build.pet_equipment ?? {})[slotKey] != null}
        <div class="tierrow">
          <span class="tierlbl">Upgrade tier</span>
          <button class="minibtn" onclick={() => setGearTier(slotKey, tierOf(slotKey) - 1)} disabled={tierOf(slotKey) <= 0}>−</button>
          <strong class="tiernum">+{tierOf(slotKey)}</strong>
          <button class="minibtn" onclick={() => setGearTier(slotKey, tierOf(slotKey) + 1)} disabled={tierOf(slotKey) >= 10}>+</button>
          <span class="tierhint">≤10: +1/tier · >10 & dmg: +10%/tier (game-exact rounding) · haste +1%/tier · delay never changes</span>
        </div>
      {/if}
      {#if petCandidates.length === 0}
        <p class="nonefound">no items match this slot (run data import)</p>
      {:else}
        <ItemPicker
          items={petCandidates}
          level={s.build.level}
          onpick={(i) => equipPet(slotKey, i)}
          placeholder={`search ${petCanon(slotKey)} items for the pet…`}
        />
      {/if}
    </div>
  {/if}
  </div>

  {#if petEditSlot != null && petEditItem != null}
    <ItemEditPopup
      {s}
      slotKey={petEditSlot}
      item={petEditItem}
      tier={tierOf(petEditSlot)}
      onclose={() => (petEditSlot = null)}
      onchangeitem={() => { openPetSlot = petEditSlot; petEditSlot = null; }}
    />
  {/if}
{/if}

<style>
  .controls { display: flex; gap: 1.2rem; align-items: center; flex-wrap: wrap; margin-bottom: .8rem; }
  label { display: flex; gap: .4rem; align-items: center; color: #9ab; }
  select { background: #1c1f26; color: #e6e6e6; border: 1px solid #333; border-radius: 6px; padding: 4px 6px; max-width: 420px; }
  .tier { display: flex; gap: .4rem; align-items: center; color: #9ab; }
  .tier strong { color: #fc6; min-width: 1.4rem; text-align: center; }
  .tier button { background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px; padding: 1px 9px; cursor: pointer; }
  .tier button:disabled { opacity: .35; cursor: default; }
  .hint { color: #667; font-size: .8rem; }
  .muted { color: #667; }
  .block { background: #1c1f26; border: 1px solid #262b33; border-radius: 8px; padding: .7rem 1rem; max-width: 950px; }
  .block.stale { opacity: .55; border-color: #a63; }
  h2 { margin: 0 0 .3rem; font-size: 1.05rem; }
  h3 { font-size: .85rem; color: #bcd; margin: .9rem 0 .3rem; }
  .unknown { color: #f90; }
  .capped {
    color: #da5; font-size: .7rem; border: 1px solid #543; border-radius: 4px;
    padding: 0 5px; cursor: help;
  }
  .estimate {
    color: #a9c; font-size: .7rem; border: 1px solid #536; border-radius: 4px;
    padding: 0 5px; cursor: help; font-style: italic;
  }
  .dim { color: #89a; }
  .chiprow { display: flex; gap: .35rem; align-items: center; margin: .25rem 0; flex-wrap: wrap; }
  .lbl { color: #89a; font-size: .72rem; min-width: 120px; }
  .chip { background: #2a6; color: #012; border-radius: 5px; padding: 1px 7px; font-size: .75rem; font-weight: 600; }
  .chip.pool { background: #46c; color: #dee; }
  .notes { list-style: none; padding: 0; margin: 0; }
  .notes li { padding: 2px 0; border-bottom: 1px solid #20242b; color: #9ab; font-size: .8rem; }

  /* ---- pet inventory (classic EQ stone panel, matches Equipment tab) ---- */
  .invhint { margin-top: 1rem; }
  /* inventory panel + the slot picker side by side (picker on the RIGHT; wraps
     below on narrow windows) */
  .invrow {
    display: flex; gap: .8rem; align-items: flex-start; flex-wrap: wrap;
    margin-top: 1rem;
  }
  .invpanel {
    flex: 0 1 760px; max-width: 760px; min-width: 420px;
    background: linear-gradient(160deg, #0d0f14, #1a1d24);
    border: 2px solid #8a7440;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px #3a3f4a, inset 0 0 28px rgba(0, 0, 0, .55);
  }
  .titlebar {
    text-align: center; font-variant: small-caps; letter-spacing: .18em;
    color: #c9b26a; font-size: .78rem; padding: 4px 8px;
    background: linear-gradient(#181b22, #12141a);
    border-bottom: 1px solid #3a3f4a;
  }
  /* the in-game arrangement: four stone bars of slots, matching the Equipment tab */
  .rows {
    display: flex; flex-direction: column; gap: 8px; padding: 10px 12px 8px;
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
  .capnote {
    margin: 8px 14px 0; padding: 5px 10px; text-align: center;
    background: #1a2118; border: 1px solid #2a6; border-radius: 4px;
    color: #7c9; font-size: .74rem;
  }
  .capnote.over { background: #211814; border-color: #a33; color: #d88; }
  .wornlist { padding: 2px 18px 4px; }
  .wornlist .lbl { color: #89a; font-size: .72rem; text-transform: uppercase; letter-spacing: .06em; }
  .wornlist ul { list-style: none; padding: 0; margin: .2rem 0 0; }
  .wornlist li { display: flex; gap: .6rem; align-items: baseline; padding: 2px 0; border-bottom: 1px solid #20242b; font-size: .82rem; flex-wrap: wrap; }
  .wornlist li.inactive { opacity: .5; }
  .wornlist .role { color: #c9b26a; font-size: .7rem; min-width: 110px; font-variant: small-caps; }
  .wornlist .wname { color: #e6e6e6; }
  .wornlist .wcat { color: #89a; font-size: .68rem; border: 1px solid #3a3f4a; border-radius: 4px; padding: 0 5px; }
  .wornlist .wproc { color: #f90; font-size: .72rem; }
  .wsum { color: #6c9; font-size: .8rem; margin: .15rem 0 .1rem; }
  .wwarn { color: #f66; font-size: .76rem; padding: 2px 0; }
  .wrule { color: #667; font-size: .68rem; font-style: italic; margin-top: .25rem; }

  /* ---- pet stats + slot slider ---- */
  .statrow { display: flex; gap: .8rem; align-items: center; flex-wrap: wrap; margin: .1rem 0 .4rem; }
  .stat { color: #cbd; font-size: .82rem; }
  .stat .sk { color: #89a; font-size: .72rem; }
  .stat strong { color: #fc6; }
  .slotctl {
    display: flex; align-items: center; gap: .6rem; flex-wrap: wrap;
    padding: 6px 14px; border-bottom: 1px solid #262b33;
  }
  .slotlbl { color: #c9b26a; font-size: .72rem; font-variant: small-caps; letter-spacing: .08em; }
  .slotctl input[type="range"] { accent-color: #c9b26a; max-width: 200px; flex: 0 1 200px; }
  .petopt {
    display: flex; align-items: center; gap: .5rem; flex-wrap: wrap;
    padding: 6px 14px; border-bottom: 1px solid #262b33;
  }
  .petoptlbl { color: #c9b26a; font-size: .72rem; font-variant: small-caps; letter-spacing: .08em; }
  .petoptbtn {
    background: #22262d; color: #cbd; border: 1px solid #3a3f4a; border-radius: 6px;
    padding: 4px 10px; cursor: pointer; font: inherit; font-size: .76rem; white-space: nowrap;
  }
  .petoptbtn:hover:not(:disabled) { background: #2a2f38; border-color: #6a7080; }
  .petoptbtn:disabled { opacity: .5; cursor: default; }
  .petoptbtn.confirm { background: #3a2a12; color: #f0b040; border-color: #b8791f; }
  .petoptbtn.clearall { background: #1c1c22; color: #99a; border-color: #445; }
  .petoptbtn.clearall:hover:not(:disabled) { background: #26262e; color: #bbc; }
  .petoptsum { color: #7c9; font-size: .72rem; }
  .slotnum { color: #fc6; min-width: 1.4rem; text-align: center; }
  .slothint { color: #778; font-size: .7rem; }
  .linkbtn {
    background: none; border: none; color: #6c9; cursor: pointer; font-size: .7rem;
    padding: 0; text-decoration: underline;
  }
  .linkbtn:hover { color: #8ea; }
  .totals {
    display: flex; gap: .4rem; align-items: center; flex-wrap: wrap;
    padding: 4px 12px 2px; justify-content: center;
  }
  .totals .lbl { color: #89a; font-size: .72rem; }
  .tchip {
    background: #12151c; border: 1px solid #3a3f4a; border-radius: 4px;
    color: #fc6; font-size: .74rem; padding: 1px 7px;
  }
  .legend { text-align: center; color: #667; font-size: .68rem; padding: 4px 8px 10px; margin: 0; }
  .lg.green { color: #2a6; }
  .lg.orange { color: #c73; }
  .lg.red { color: #a33; }

  /* tooltip content (rendered inside SlotWell's tip panel) */
  .tipname { color: #c9b26a; font-weight: 600; margin-bottom: 2px; }
  .tipbadge { font-size: .68rem; display: inline-block; border: 1px solid currentColor; border-radius: 4px; padding: 0 4px; margin: 1px 0; }
  .b-FULLY_ACTIVE { color: #2a6; }
  .b-PROC_INACTIVE { color: #c73; }
  .b-INVALID_CLASS { color: #f66; }
  .b-OUT_OF_ERA { color: #f66; }
  .b-OVER_CAP { color: #f66; }
  .tipreason { color: #da5; }
  .tipempty { color: #667; font-style: italic; }

  .pickerbox {
    flex: 1 1 340px; max-width: 480px; min-width: 300px;
    position: sticky; top: .5rem;
    margin: 0; padding: .5rem;
    background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
  }
  .pickhead {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: .4rem; color: #9ab; font-size: .85rem;
  }
  .minibtn {
    background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px;
    padding: 1px 8px; cursor: pointer; font-size: .75rem;
  }
  .minibtn:hover:not(:disabled) { color: #e6e6e6; border-color: #2a6; }
  .minibtn:disabled { opacity: .35; cursor: default; }
  .minibtn.edit { color: #c9b26a; border-color: #8a7440; }
  .minibtn.edit:hover { color: #e6d08a; border-color: #c9b26a; }
  .pickbtns { display: flex; gap: .4rem; align-items: center; }

  .cathint { color: #667; font-size: .68rem; margin-left: .3rem; }
  .pickhead strong { color: #c9b26a; }
  .nonefound { color: #667; font-style: italic; margin: .3rem 0; }
  .tierrow {
    display: flex; gap: .5rem; align-items: center; margin-bottom: .45rem;
    padding: .3rem .5rem; background: #12151c; border: 1px solid #3a3f4a; border-radius: 6px;
  }
  .tierlbl { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .tiernum { color: #c9b26a; min-width: 2rem; text-align: center; }
  .tierhint { color: #667; font-size: .68rem; margin-left: auto; }
</style>
