<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import type { AppState } from "../state.svelte";
  import {
    focusClient, querySpells, spellIcons, spellLines, exportSpellbook, importSpellbook,
    listLoadoutFiles, exportSpellbookToGame,
    type FocusClient, type LoadoutFile, type SpellLoadout, type SpellRow,
  } from "../api";
  import {
    hideOnError, spellIconUrl, spellTierMana, spellTierTime, spellTierValue,
  } from "../format";
  import { aaRank, findAaByName, MNEMONIC_AA, setAaRank } from "../aa";
  import SlotWell from "../SlotWell.svelte";

  let { s }: { s: AppState } = $props();

  // in-game capacity: 50 pages x 8 squares = 400 squares, shown as 25 spreads
  // of 16 (2 pages x 8). Absolute square index 0..399.
  const SQUARES = 400;
  const PER_PAGE = 8;
  const PER_SPREAD = PER_PAGE * 2;
  const SPREADS = SQUARES / PER_SPREAD; // 25
  const PAGES = SQUARES / PER_PAGE;     // 50

  // memorized spell bar: 8 base gems + 1 per rank of the AA Mnemonic Retention (6 ranks).
  // The game file always writes 14 slots per set, and stores up to 60 sets.
  const BASE_GEMS = 8;
  const MAX_GEMS = 14;
  const MAX_RANK = 6;
  const MAX_SETS = 60;
  const GEM_IDX = Array.from({ length: MAX_GEMS }, (_, i) => i);

  let spread = $state(0);           // 0..SPREADS-1
  let openSquare = $state<number | null>(null);
  let pickSearch = $state("");
  let flash = $state<string | null>(null);

  // ---- available spells for the build (merged one-row-per-id, like the Spells tab) ----
  interface MergedSpell {
    base: SpellRow;
    entries: { cls: string; level: number }[];
    minLevel: number;
  }
  let spells = $state<SpellRow[]>([]);
  let loading = $state(false);
  let gen = 0;
  $effect(() => {
    const classes = [...s.build.classes];
    const level = s.build.level;
    const g = ++gen;
    if (classes.length === 0) { spells = []; loading = false; return; }
    loading = true;
    const t = setTimeout(() => {
      querySpells(classes, level)
        .then((r) => { if (g === gen) { spells = r; loading = false; } })
        .catch((e) => { if (g === gen) { loading = false; s.error = String(e); } });
    }, 250);
    return () => clearTimeout(t);
  });

  const merged = $derived.by(() => {
    const map = new Map<number, MergedSpell>();
    for (const r of spells) {
      let m = map.get(r.id);
      if (!m) { m = { base: r, entries: [], minLevel: r.required_class_level }; map.set(r.id, m); }
      m.entries.push({ cls: r.class, level: r.required_class_level });
      m.minLevel = Math.min(m.minLevel, r.required_class_level);
    }
    return [...map.values()].sort(
      (a, b) => a.minLevel - b.minLevel || a.base.name.localeCompare(b.base.name)
    );
  });
  const availById = $derived(new Map(merged.map((m) => [m.base.id, m])));

  // ---- gem icons: union of available + scribed + memorized ids (covers imported spells) ----
  let iconMap = $state<Record<number, string>>({});
  let iconGen = 0;
  $effect(() => {
    const ids = new Set<number>();
    for (const m of merged) ids.add(m.base.id);
    for (const v of Object.values(s.build.spellbook ?? {})) ids.add(v);
    for (const lo of s.build.loadouts ?? []) {
      for (const id of lo.slots) if (id != null) ids.add(id);
    }
    const list = [...ids];
    const g = ++iconGen;
    if (list.length === 0) { iconMap = {}; return; }
    spellIcons(list)
      .then((r) => { if (g === iconGen) iconMap = r; })
      .catch(() => { /* icons are decorative; ignore */ });
  });
  function iconFor(id: number): string | null {
    return iconMap[id] ?? availById.get(id)?.base.icon ?? null;
  }
  function nameFor(id: number): string {
    return availById.get(id)?.base.name ?? `spell #${id}`;
  }
  function isSummon(id: number): boolean {
    return availById.get(id)?.base.is_summon ?? false;
  }

  // ---- scribe / clear / tier (spell_tiers is the SAME map the Spells tab writes) ----
  function scribe(idx: number, id: number) {
    s.build.spellbook = { ...(s.build.spellbook ?? {}), [idx]: id };
  }
  function unscribe(idx: number) {
    const next = { ...(s.build.spellbook ?? {}) };
    delete next[idx];
    s.build.spellbook = next;
  }
  function tierOf(id: number): number {
    return s.build.spell_tiers?.[id] ?? 0;
  }
  function setTier(id: number, n: number) {
    const clamped = Math.min(10, Math.max(0, n));
    const next = { ...(s.build.spell_tiers ?? {}) };
    if (clamped === 0) delete next[id];
    else next[id] = clamped;
    s.build.spell_tiers = next;
  }

  const pickList = $derived(
    merged
      .filter((m) => !pickSearch || m.base.name.toLowerCase().includes(pickSearch.toLowerCase()))
      .slice(0, 200)
  );

  // ---- worn FOCUS effects applied to the displayed numbers (user spec 2026-07-21):
  // equipped focus items + socketed Focus Exaltations modify mana / cast / dmg / heal.
  // Limits are now EXACT — decoded from the client's own effect slots (focus_client):
  // Max Level (with per-level decay), Min Level, and spell Type (beneficial/detrimental).
  interface FocusPower extends FocusClient {
    name: string;
  }
  let focusPowers = $state<FocusPower[]>([]);
  let focusGen = 0;
  $effect(() => {
    const fx = (s.result?.effect_overview ?? []).filter(
      (e) => e.kind === "FOCUS" && !e.level_gated && e.spell_id != null
    );
    const ids = [...new Set(fx.map((e) => e.spell_id as number))];
    const g = ++focusGen;
    if (ids.length === 0) { focusPowers = []; return; }
    focusClient(ids)
      .then((rows) => {
        if (g !== focusGen) return;
        focusPowers = rows.map((r) => ({
          ...r,
          name: fx.find((e) => e.spell_id === r.spell_id)?.effect_name ?? `focus #${r.spell_id}`,
        }));
      })
      .catch(() => { /* informational overlay; ignore fetch errors */ });
  });
  // focus kind matched by the OLD opcode names spellNumbers() passes in
  const KIND_FOR: Record<string, FocusClient["kind"]> = {
    FOCUS_MANA_COST: "MANA", SPELL_HASTE: "HASTE",
    FOCUS_SPELL_DAMAGE: "DMG", FOCUS_HEALING: "HEAL",
  };

  /** the focus effectiveness (0..1) for a focused spell at `lvl`, honoring Max Level +
   *  per-level decay: full up to max_level, then loses level_decay_pct per level over. */
  function focusEffectiveness(f: FocusClient, lvl: number | null): number {
    if (f.min_level != null && (lvl == null || lvl < f.min_level)) return 0;
    if (f.max_level == null || lvl == null || lvl <= f.max_level) return 1;
    const decay = f.level_decay_pct ?? 100; // no decay value = hard cutoff
    return Math.max(0, 1 - (decay / 100) * (lvl - f.max_level));
  }

  /** strongest applicable worn focus of a kind for a spell (exact level + type limits).
   *  Returns the effective percentage already scaled by Max-Level decay. */
  function bestFocus(op: string, lvl: number | null, beneficial: boolean):
      { name: string; pct: number } | null {
    const kind = KIND_FOR[op];
    if (!kind) return null;
    let best: { name: string; pct: number } | null = null;
    for (const f of focusPowers) {
      if (f.kind !== kind) continue;
      if (f.beneficial_only && !beneficial) continue;
      if (f.detrimental_only && beneficial) continue;
      const eff = focusEffectiveness(f, lvl);
      if (eff <= 0) continue;
      const pct = f.pct_max * eff;
      if (best == null || pct > best.pct) best = { name: f.name, pct };
    }
    return best;
  }

  /** displayed numbers for a spell: upgrade tier first, then worn focus effects */
  function spellNumbers(m: SpellRow, id: number) {
    const tier = tierOf(id);
    const lvlRaw = levelFor(id, null);
    const lvl = lvlRaw >= 999 ? null : lvlRaw;
    const sPct = s.staticData?.spell_tier_scaling_pct ?? 6;
    const mPct = s.staticData?.spell_tier_mana_pct ?? 6;
    const mFloor = s.staticData?.spell_tier_mana_floor ?? 20;
    const cPct = s.staticData?.spell_tier_cast_pct ?? 4;
    const notes: string[] = [];
    const pctInt = (p: number) => Math.round(p);
    let mana = m.mana;
    if (mana != null) {
      const tiered = mana >= mFloor ? spellTierMana(mana, tier, mPct) : mana;
      const f = bestFocus("FOCUS_MANA_COST", lvl, m.is_beneficial);
      const final = f != null ? Math.round(tiered * (1 - f.pct / 100)) : tiered;
      if (f != null) notes.push(`${f.name}: −${pctInt(f.pct)}% mana`);
      mana = final;
    }
    let cast = m.casting_time;
    if (cast != null) {
      let c = spellTierTime(cast, tier, cPct);
      const f = bestFocus("SPELL_HASTE", lvl, m.is_beneficial);
      if (f != null) { c = c * (1 - f.pct / 100); notes.push(`${f.name}: −${pctInt(f.pct)}% cast time`); }
      cast = c;
    }
    const scale = (v: number | null, f: { pct: number } | null): number | null => {
      if (v == null) return null;
      const t = spellTierValue(v, tier, sPct);
      return f != null ? Math.floor(t * (1 + f.pct / 100)) : t;
    };
    const fd = m.dmg_max != null ? bestFocus("FOCUS_SPELL_DAMAGE", lvl, false) : null;
    if (fd != null) notes.push(`${fd.name}: +${pctInt(fd.pct)}% damage`);
    const fh = m.heal_max != null ? bestFocus("FOCUS_HEALING", lvl, true) : null;
    if (fh != null) notes.push(`${fh.name}: +${pctInt(fh.pct)}% healing`);
    return {
      tier,
      mana,
      manaChanged: mana != null && mana !== m.mana,
      cast,
      castChanged: cast != null && cast !== m.casting_time,
      dmgMin: scale(m.dmg_min, fd), dmgMax: scale(m.dmg_max, fd),
      dmgChanged: fd != null || (tier > 0 && m.dmg_max != null),
      healMin: scale(m.heal_min, fh), healMax: scale(m.heal_max, fh),
      healChanged: fh != null || (tier > 0 && m.heal_max != null),
      notes,
    };
  }

  let hoverSq = $state<number | null>(null); // book square index under the cursor

  function scribedIdAt(idx: number): number | null {
    const v = (s.build.spellbook ?? {})[idx];
    return v == null ? null : v;
  }
  function leftPage(sp: number): number[] {
    const start = sp * PER_SPREAD;
    return Array.from({ length: PER_PAGE }, (_, i) => start + i);
  }
  function rightPage(sp: number): number[] {
    const start = sp * PER_SPREAD + PER_PAGE;
    return Array.from({ length: PER_PAGE }, (_, i) => start + i);
  }

  function showFlash(msg: string) {
    flash = msg;
    setTimeout(() => { if (flash === msg) flash = null; }, 4000);
  }

  // ---- export / import ----
  function synthLoadout(): SpellLoadout[] {
    if ((s.build.loadouts ?? []).length > 0) return s.build.loadouts;
    const slots: (number | null)[] = [];
    for (let i = 0; i < SQUARES && slots.length < 14; i++) {
      const id = scribedIdAt(i);
      if (id != null) slots.push(id);
    }
    while (slots.length < 14) slots.push(null);
    return [{ name: s.build.name, slots }];
  }
  async function onExport() {
    try {
      const path = await exportSpellbook(s.build.name, synthLoadout());
      showFlash(`exported -> ${path}`);
    } catch (e) { s.error = String(e); }
  }
  async function onImport() {
    try {
      const path = await open({ filters: [{ name: "INI", extensions: ["ini"] }] });
      if (path == null || Array.isArray(path)) return; // cancelled / multi-select
      const loadouts = await importSpellbook(path);
      s.build.loadouts = loadouts;
      // scribe every referenced pageid into book squares in order
      const book: Record<number, number> = {};
      let idx = 0;
      for (const lo of loadouts) {
        for (const id of lo.slots) {
          if (id != null && idx < SQUARES) { book[idx] = id; idx++; }
        }
      }
      s.build.spellbook = book;
      spread = 0;
      showFlash(`imported ${loadouts.length} loadout(s), ${idx} spell(s) scribed`);
    } catch (e) { s.error = String(e); }
  }

  // ---- write spell sets INTO a live game file (safe in-place merge) ----
  let showWrite = $state(false);
  let loadoutFiles = $state<LoadoutFile[]>([]);
  let writeBusy = $state(false);
  let confirmWrite = $state<string | null>(null); // path awaiting a second-click confirm

  async function toggleWrite() {
    showWrite = !showWrite;
    confirmWrite = null;
    if (showWrite) {
      try { loadoutFiles = await listLoadoutFiles(); }
      catch (e) { s.error = String(e); }
    }
  }
  async function writeToGame(path: string) {
    // two-click confirm: first click arms, second click writes (it edits a live game file)
    if (confirmWrite !== path) { confirmWrite = path; return; }
    confirmWrite = null; writeBusy = true;
    try {
      const r = await exportSpellbookToGame(path, synthLoadout());
      const bak = r.backup ? ` · backup: ${r.backup.split(/[\\/]/).pop()}` : "";
      const warn = r.slots_unresolved > 0 ? ` · ⚠ ${r.slots_unresolved} gem(s) had no game id (left empty)` : "";
      showFlash(`wrote ${r.sets_written} set(s) to ${path.split(/[\\/]/).pop()}${bak}${warn}`);
      showWrite = false;
    } catch (e) { s.error = String(e); }
    finally { writeBusy = false; }
  }
  async function browseWriteTarget() {
    try {
      const path = await open({ filters: [{ name: "Loadout INI", extensions: ["ini"] }] });
      if (typeof path === "string") confirmWrite = path; // arm confirm for the chosen file
    } catch (e) { s.error = String(e); }
  }

  const scribedCount = $derived(Object.keys(s.build.spellbook ?? {}).length);

  // ---- spell sets = the MEMORIZED SPELL BAR (the game's [SpellLoadouts]) ----
  // NOT the spellbook: the book is what's scribed, a set is which 14 gems are memorized.
  const loadouts = $derived(s.build.loadouts ?? []);
  /** gems this build actually has; the engine computes it (8 + Mnemonic Retention rank). */
  const gemCount = $derived(s.result?.spell_gem_count ?? BASE_GEMS);

  // Mnemonic Retention is a normal AA: this stepper is a shortcut into the AA planner
  // (build.aa_ranks), the same map the AA tab writes — ONE source of truth. The legacy
  // standalone field is only used if the AA list never loaded.
  const mnemAa = $derived(findAaByName(s.aas, MNEMONIC_AA));
  const maxRank = $derived(mnemAa?.max_rank ?? MAX_RANK);
  const mnemonic = $derived(
    mnemAa ? aaRank(s.build, mnemAa.id) : (s.build.aa_mnemonic_retention ?? 0)
  );

  function setMnemonic(n: number) {
    if (mnemAa) setAaRank(s.build, mnemAa, n);
    else s.build.aa_mnemonic_retention = Math.min(MAX_RANK, Math.max(0, n));
  }

  /** the file always holds 14 slots per set — normalize whatever we were handed */
  function slots14(src: (number | null)[]): (number | null)[] {
    const out = src.slice(0, MAX_GEMS);
    while (out.length < MAX_GEMS) out.push(null);
    return out;
  }
  function slotAt(lo: SpellLoadout, j: number): number | null {
    return lo.slots[j] ?? null;
  }
  /** spells sitting in gems this build HAS */
  function memorizedCount(lo: SpellLoadout): number {
    let n = 0;
    for (let j = 0; j < gemCount; j++) if (slotAt(lo, j) != null) n++;
    return n;
  }
  /** spells parked in gems this build does NOT have yet (imported set, low AA rank) */
  function lockedCount(lo: SpellLoadout): number {
    let n = 0;
    for (let j = gemCount; j < MAX_GEMS; j++) if (slotAt(lo, j) != null) n++;
    return n;
  }
  /** every mutation rebuilds the loadout array + the touched set, so the pipeline re-fires */
  function writeLoadouts(next: SpellLoadout[]) {
    s.build.loadouts = next;
  }
  function renameLoadout(i: number, name: string) {
    writeLoadouts(loadouts.map((lo, k) => (k === i ? { ...lo, name, slots: [...lo.slots] } : lo)));
  }
  function setGem(i: number, j: number, id: number | null) {
    writeLoadouts(
      loadouts.map((lo, k) => {
        if (k !== i) return lo;
        const slots = slots14(lo.slots);
        slots[j] = id;
        return { ...lo, slots };
      })
    );
  }
  function addLoadout() {
    if (loadouts.length >= MAX_SETS) return;
    const next: SpellLoadout = {
      name: `Set ${loadouts.length + 1}`,
      slots: Array<number | null>(MAX_GEMS).fill(null),
    };
    writeLoadouts([...loadouts, next]);
  }
  function duplicateLoadout(i: number) {
    if (loadouts.length >= MAX_SETS) return;
    const src = loadouts[i];
    const copy: SpellLoadout = { name: `${src.name} copy`, slots: slots14(src.slots) };
    writeLoadouts([...loadouts.slice(0, i + 1), copy, ...loadouts.slice(i + 1)]);
  }
  function deleteLoadout(i: number) {
    writeLoadouts(loadouts.filter((_, k) => k !== i));
    if (openGem?.lo === i) openGem = null;
    else if (openGem != null && openGem.lo > i) openGem = { lo: openGem.lo - 1, slot: openGem.slot };
  }

  // ---- gem picker (scribed spells first; "show all" widens it to everything castable) ----
  let openGem = $state<{ lo: number; slot: number } | null>(null);
  let gemSearch = $state("");
  let gemShowAll = $state(false);

  /** scribed spell ids in book order (that's the order the user arranged the book in) */
  const scribedIds = $derived(
    Object.entries(s.build.spellbook ?? {})
      .sort((a, b) => Number(a[0]) - Number(b[0]))
      .map(([, id]) => id)
      .filter((id, i, arr) => arr.indexOf(id) === i)
  );
  const gemScribed = $derived(
    scribedIds.filter(
      (id) => !gemSearch || nameFor(id).toLowerCase().includes(gemSearch.toLowerCase())
    ).slice(0, 200)
  );
  const gemOther = $derived.by(() => {
    if (!gemShowAll) return [] as number[];
    const inBook = new Set(scribedIds);
    return merged
      .filter((m) => !inBook.has(m.base.id))
      .filter((m) => !gemSearch || m.base.name.toLowerCase().includes(gemSearch.toLowerCase()))
      .map((m) => m.base.id)
      .slice(0, 200);
  });

  function openGemPicker(i: number, j: number) {
    if (j >= gemCount) return; // locked gem: needs more Mnemonic Retention
    openSquare = null;         // one picker at a time
    openGem = openGem?.lo === i && openGem?.slot === j ? null : { lo: i, slot: j };
    gemSearch = "";
  }
  function pickGem(id: number) {
    if (!openGem) return;
    setGem(openGem.lo, openGem.slot, id);
    openGem = null;
    gemSearch = "";
  }
  /** class/level line for a gem-picker row (blank for imported spells this build can't cast) */
  function castLine(id: number): string {
    const m = availById.get(id);
    if (!m) return "not castable by this build";
    return m.entries.map((e) => `${e.cls} ${e.level}`).join(" / ");
  }

  // ---- Feature A: auto-organize (class-first grouping x by level | by category | by line) ----
  type SortMode = "level" | "category" | "line";
  let sortMode = $state<SortMode>("level");
  let groupByClass = $state(true);
  let lineMap = $state<Record<number, string> | null>(null);
  let linePromise: Promise<Record<number, string>> | null = null;
  async function getLines(): Promise<Record<number, string>> {
    if (lineMap) return lineMap;
    if (!linePromise) {
      linePromise = spellLines().then((r) => { lineMap = r; return r; });
    }
    return linePromise;
  }

  /** SpellRow.role buckets, in book order; anything else (incl. null) sorts last. */
  const ROLE_ORDER = ["PET_SUMMON", "BUFF", "PET_BUFF", "DAMAGE", "CONTROL", "UTILITY"];
  function roleRank(id: number): number {
    const role = availById.get(id)?.base.role ?? null;
    const i = role == null ? -1 : ROLE_ORDER.indexOf(role);
    return i >= 0 ? i : ROLE_ORDER.length; // unknown / null last
  }

  /**
   * Required level for the sort. Inside a class block that's the level for THAT class;
   * ungrouped it's the min across the build's classes. Unknown spells (imported, not in
   * the available list) sort to the end.
   */
  function levelFor(id: number, cls: string | null): number {
    const m = availById.get(id);
    if (!m) return 999;
    if (cls != null) {
      const e = m.entries.find((x) => x.cls === cls);
      if (e) return e.level;
    }
    return m.minLevel;
  }
  function cmpLevel(a: number, b: number, cls: string | null): number {
    return levelFor(a, cls) - levelFor(b, cls) || nameFor(a).localeCompare(nameFor(b));
  }
  /** family = buff line (or the spell's own name); families by (min level, name), members by level then name */
  function sortIdsByLine(ids: number[], lines: Record<number, string>, cls: string | null): number[] {
    const famOf = (id: number) => lines[id] ?? nameFor(id);
    const famMin = new Map<string, number>();
    for (const id of ids) {
      const f = famOf(id);
      famMin.set(f, Math.min(famMin.get(f) ?? 999, levelFor(id, cls)));
    }
    return [...ids].sort((a, b) => {
      const fa = famOf(a), fb = famOf(b);
      if (fa !== fb) {
        const d = (famMin.get(fa) ?? 999) - (famMin.get(fb) ?? 999);
        return d !== 0 ? d : fa.localeCompare(fb);
      }
      return cmpLevel(a, b, cls);
    });
  }
  /** the chosen sort, applied within one block (a class block, or the whole book) */
  async function sortBlock(ids: number[], cls: string | null): Promise<number[]> {
    if (sortMode === "line") return sortIdsByLine(ids, await getLines(), cls);
    if (sortMode === "category") {
      return [...ids].sort((a, b) => roleRank(a) - roleRank(b) || cmpLevel(a, b, cls));
    }
    return [...ids].sort((a, b) => cmpLevel(a, b, cls));
  }

  /** first build class that can cast this spell (so a shared spell is never duplicated) */
  function classOf(id: number): string | null {
    const m = availById.get(id);
    if (!m) return null;
    for (const c of s.build.classes) {
      if (m.entries.some((e) => e.cls === c)) return c;
    }
    return null;
  }

  interface Block { cls: string | null; ids: number[] }
  /** one sorted block per build class (class order = build order, unknowns last), or a single block */
  async function orderedBlocks(ids: number[]): Promise<Block[]> {
    if (!groupByClass) return [{ cls: null, ids: await sortBlock(ids, null) }];
    const buckets = new Map<string | null, number[]>();
    for (const c of s.build.classes) buckets.set(c, []);
    buckets.set(null, []); // insertion order => unknown-class spells land last
    for (const id of ids) buckets.get(classOf(id))!.push(id);
    const out: Block[] = [];
    for (const [cls, list] of buckets) {
      if (list.length > 0) out.push({ cls, ids: await sortBlock(list, cls) });
    }
    return out;
  }

  const sortLabel = $derived(
    sortMode === "level" ? "level" : sortMode === "category" ? "category" : "line"
  );
  const organizeLabel = $derived(groupByClass ? `by class, then ${sortLabel}` : `by ${sortLabel}`);

  async function autoOrganize() {
    const ids = Object.entries(s.build.spellbook ?? {})
      .sort((a, b) => Number(a[0]) - Number(b[0]))
      .map(([, id]) => id);
    if (ids.length === 0) return;
    try {
      const blocks = await orderedBlocks(ids);
      const book: Record<number, number> = {};
      let idx = 0, placed = 0, overflow = 0;
      for (const b of blocks) {
        // class-tabbed book: each class starts on a fresh page (previous page padded empty)
        if (groupByClass && idx % PER_PAGE !== 0) idx += PER_PAGE - (idx % PER_PAGE);
        for (const id of b.ids) {
          if (idx >= SQUARES) { overflow++; continue; }
          book[idx++] = id;
          placed++;
        }
      }
      s.build.spellbook = book;
      spread = 0;
      showFlash(
        `organized ${placed} spells — ${organizeLabel}` +
        (overflow > 0 ? ` · ${overflow} didn't fit` : "")
      );
    } catch (e) { s.error = String(e); }
  }

  // ---- global tier override (click-only; dragging must not spam the resolve pipeline) ----
  let bulkTier = $state(0);
  function applyAllTiers() {
    const ids = [...new Set(Object.values(s.build.spellbook ?? {}))];
    if (ids.length === 0) return;
    const next = { ...(s.build.spell_tiers ?? {}) };
    for (const id of ids) {
      if (bulkTier === 0) delete next[id]; // tier 0 = absent, never a stored zero
      else next[id] = bulkTier;
    }
    s.build.spell_tiers = next;
    showFlash(`set ${ids.length} spells to tier ${bulkTier}`);
  }

  // ---- Feature B: add all class spells / clear book ----
  let classToggles = $state<string[]>([]);
  $effect(() => {
    classToggles = [...s.build.classes]; // default all ON; reset when the class combo changes
  });
  function toggleCls(c: string) {
    classToggles = classToggles.includes(c)
      ? classToggles.filter((x) => x !== c)
      : [...classToggles, c];
  }

  async function addAllSpells() {
    try {
      const scribedIds = new Set(Object.values(s.build.spellbook ?? {}));
      const candidates = merged.filter((m) => m.entries.some((e) => classToggles.includes(e.cls)));
      const toAdd = candidates.filter((m) => !scribedIds.has(m.base.id)).map((m) => m.base.id);
      const skipped = candidates.length - toAdd.length;
      // fills the book's existing gaps, so blocks stay contiguous (no page padding here —
      // run Auto-organize afterwards for the class-tabbed layout)
      const ordered = (await orderedBlocks(toAdd)).flatMap((b) => b.ids);
      const book = { ...(s.build.spellbook ?? {}) };
      let placed = 0;
      let idx = 0;
      for (const id of ordered) {
        while (idx < SQUARES && book[idx] != null) idx++;
        if (idx >= SQUARES) break;
        book[idx] = id;
        placed++;
      }
      const overflow = ordered.length - placed; // guard; nearly unreachable at 400 squares
      s.build.spellbook = book;
      showFlash(
        `scribed ${placed} spells (${skipped} skipped, already scribed)` +
        (overflow > 0 ? ` · +${overflow} didn't fit` : "")
      );
    } catch (e) { s.error = String(e); }
  }

  let confirmClear = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;
  function onClearBook() {
    if (!confirmClear) {
      confirmClear = true;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmClear = false), 3000);
      return;
    }
    clearTimeout(confirmTimer);
    confirmClear = false;
    s.build.spellbook = {};
    showFlash("spellbook cleared");
  }
</script>

{#snippet spellTip(id: number)}
  {@const m = availById.get(id)?.base}
  <div class="tipname">{nameFor(id)}{tierOf(id) > 0 ? ` +${tierOf(id)}` : ""}</div>
  {#if m}
    {@const n = spellNumbers(m, id)}
    <div class="tiprow dim">{castLine(id)}</div>
    {#if n.mana != null}
      <div class="tiprow">Mana: <span class:up={n.manaChanged}>{n.mana}</span>{#if n.manaChanged}<span class="approx">≈</span>{/if}</div>
    {/if}
    {#if n.cast != null}
      <div class="tiprow">Cast: <span class:up={n.castChanged}>{n.cast.toFixed(2)}s</span></div>
    {/if}
    {#if n.dmgMax != null}
      <div class="tiprow">Damage: <span class:up={n.dmgChanged}>{n.dmgMin ?? n.dmgMax}–{n.dmgMax}</span>{#if n.dmgChanged}<span class="approx">≈</span>{/if}</div>
    {/if}
    {#if n.healMax != null}
      <div class="tiprow">Healing: <span class:up={n.healChanged}>{n.healMin ?? n.healMax}–{n.healMax}</span>{#if n.healChanged}<span class="approx">≈</span>{/if}</div>
    {/if}
    {#if m.duration}<div class="tiprow">Duration: {m.duration}</div>{/if}
    {#each n.notes as note, i (i)}
      <div class="tipfocus">✦ {note}</div>
    {/each}
    {#if n.notes.length > 0}
      <div class="tipapprox">focus shows the max roll (client-exact level + type limits)</div>
    {/if}
    {#if m.resolved_description}<div class="tipdesc">{m.resolved_description}</div>{/if}
  {:else}
    <div class="tiprow dim">not castable by this build — no spell data</div>
  {/if}
{/snippet}

{#snippet square(idx: number)}
  {@const id = scribedIdAt(idx)}
  {@const icon = id != null ? iconFor(id) : null}
  <div class="sqwrap">
    <button
      class="sq"
      class:filled={id != null}
      class:sel={openSquare === idx}
      onclick={() => { openGem = null; openSquare = openSquare === idx ? null : idx; }}
      onmouseenter={() => (hoverSq = idx)}
      onmouseleave={() => (hoverSq = hoverSq === idx ? null : hoverSq)}
      onfocus={() => (hoverSq = idx)}
      onblur={() => (hoverSq = hoverSq === idx ? null : hoverSq)}
    >
      {#if id != null}
        {#if icon != null}
          {#key icon}
            <img src={spellIconUrl(icon)} alt="" draggable="false" onerror={hideOnError} />
          {/key}
        {/if}
        <span class="sqname">{nameFor(id)}</span>
        {#if tierOf(id) > 0}<span class="sqtier">+{tierOf(id)}</span>{/if}
        {#if levelFor(id, null) < 999}
          <span class="sqlvl" title="required level to cast">L{levelFor(id, null)}</span>
        {/if}
      {:else}
        <span class="sqempty">{idx + 1}</span>
      {/if}
    </button>
    {#if hoverSq === idx && id != null}
      <div class="stip">{@render spellTip(id)}</div>
    {/if}
  </div>
{/snippet}

<div class="toolbar">
  <button class="tb" onclick={onExport} title="save a shareable [SpellLoadouts] fragment to your Desktop">Export to Desktop</button>
  <button class="tb" onclick={onImport}>Import…</button>
  <button class="tb" class:on={showWrite} onclick={toggleWrite} title="merge these spell sets into a live game LO1.ini, preserving everything else">Write to game…</button>
  <span class="sep"></span>
  <select class="sortsel" bind:value={sortMode} title="sort order used by Auto-organize and Add all spells">
    <option value="level">by level</option>
    <option value="category">by category</option>
    <option value="line">by line</option>
  </select>
  <label class="chk" title="group spells by build class first — each class starts on a fresh page">
    <input type="checkbox" bind:checked={groupByClass} />
    group by class
  </label>
  <button class="tb" onclick={autoOrganize} disabled={scribedCount === 0} title="rewrite the book {organizeLabel}">Auto-organize</button>
  <span class="sep"></span>
  <!-- dedupe: a build may legitimately hold the same class more than once, and a keyed
       each hard-throws on duplicate keys (and three identical chips help nobody) -->
  {#each [...new Set(s.build.classes)] as c (c)}
    <button class="clschip" class:on={classToggles.includes(c)} onclick={() => toggleCls(c)} title="include {c} spells in Add all">{c}</button>
  {/each}
  <button class="tb" onclick={addAllSpells} disabled={classToggles.length === 0 || loading}>Add all spells</button>
  <button
    class="tb" class:cleardanger={confirmClear}
    onclick={onClearBook}
    disabled={scribedCount === 0 && !confirmClear}
  >{confirmClear ? "really clear?" : "Clear book"}</button>
  <span class="sccount">{scribedCount} scribed{loading ? " · loading spells…" : ""}</span>
  {#if flash}<span class="flash">{flash}</span>{/if}
</div>

{#if showWrite}
  <div class="writepanel">
    <p class="wtitle">Write spell sets into a live game file</p>
    <p class="wnote">
      Merges this build's sets into the character's <code>&lt;Char&gt;_&lt;city&gt;_LO1.ini</code>.
      <strong>Everything else in that file — hotbars, socials, sound — is preserved</strong>, and a
      <code>.bak</code> backup is saved first. Close EQL before writing so the game doesn't overwrite it.
    </p>
    {#if loadoutFiles.length === 0}
      <p class="wmuted">No <code>&lt;Char&gt;_&lt;city&gt;_LO1.ini</code> found in the EQL folder — use Browse.</p>
    {/if}
    <ul class="wlist">
      {#each loadoutFiles as f (f.path)}
        <li>
          <span class="wf">{f.character ?? "?"} · {f.city ?? "?"} <em>({f.set_count} in use)</em></span>
          <button class="tb sm" class:arm={confirmWrite === f.path} onclick={() => writeToGame(f.path)} disabled={writeBusy}>
            {confirmWrite === f.path ? "click again to write" : "Write here"}
          </button>
        </li>
      {/each}
      {#if confirmWrite && !loadoutFiles.some((f) => f.path === confirmWrite)}
        <li>
          <span class="wf" title={confirmWrite}>{confirmWrite.split(/[\\/]/).pop()}</span>
          <button class="tb sm arm" onclick={() => confirmWrite && writeToGame(confirmWrite)} disabled={writeBusy}>click again to write</button>
        </li>
      {/if}
    </ul>
    <button class="tb sm" onclick={browseWriteTarget}>Browse…</button>
  </div>
{/if}

<div class="tierbar">
  <div class="tierctl">
    <label class="tblabel" for="bulktier">Set all tiers</label>
    <input id="bulktier" class="tslider" type="range" min="0" max="10" step="1" bind:value={bulkTier} />
    <strong class="tval">+{bulkTier}</strong>
    <button class="tb" onclick={applyAllTiers} disabled={scribedCount === 0}>Apply to all</button>
  </div>
  <p class="tnote">
    tiers can't be imported — the game doesn't store spell upgrade level in any local file; set them here.
  </p>
</div>

<p class="note">
  Your spellbook drives the Buffs tab's <em>Strict availability</em> mode — with it on,
  only spells scribed here can self-cast. Summons scribed here scale the Pet tab
  (tier raises pet level + stats).
</p>

<section class="sets">
  <div class="setshead">
    <div class="settitle">
      <h3>Spell Sets <span class="dim">(memorized gems)</span></h3>
      <p class="setsnote">
        your memorized spell bar — 14 gems max (8 base + Mnemonic Retention AA).
        These are the sets the game stores; the book below is what's scribed.
      </p>
    </div>
    <div class="aa" title="AA: Mnemonic Retention — +1 spell gem per rank. Same rank as on the AA tab.">
      <span class="aalbl">Mnemonic Retention</span>
      <button class="mini" onclick={() => setMnemonic(mnemonic - 1)} disabled={mnemonic <= 0}>−</button>
      <strong class="aanum">{mnemonic}</strong>
      <button class="mini" onclick={() => setMnemonic(mnemonic + 1)} disabled={mnemonic >= maxRank}>+</button>
      <span class="aahint">rank 0–{maxRank} · <strong>{gemCount}</strong> gems · costs AA points (AA tab)</span>
    </div>
  </div>

  {#each loadouts as lo, i (i)}
    <div class="setrow">
      <div class="sethead">
        <input
          class="setname"
          value={lo.name}
          maxlength="32"
          placeholder="set name"
          aria-label="spell set name"
          oninput={(e) => renameLoadout(i, e.currentTarget.value)}
        />
        <span class="setcount">{memorizedCount(lo)} / {gemCount} memorized</span>
        {#if lockedCount(lo) > 0}
          <span class="setwarn">{lockedCount(lo)} in locked gems</span>
        {/if}
        <span class="spacer"></span>
        <button class="mini" onclick={() => duplicateLoadout(i)} disabled={loadouts.length >= MAX_SETS}>Duplicate</button>
        <button class="mini danger" onclick={() => deleteLoadout(i)}>Delete</button>
      </div>

      <div class="gemrow">
        {#each GEM_IDX as j (j)}
          {@const gid = slotAt(lo, j)}
          {@const locked = j >= gemCount}
          {@const gicon = gid != null ? iconFor(gid) : null}
          <SlotWell
            iconId={null}
            iconSrc={gicon != null ? spellIconUrl(gicon) : null}
            label={String(j + 1)}
            filled={!locked && gid != null}
            {locked}
            tier={!locked && gid != null ? tierOf(gid) : 0}
            cornerLabel={!locked && gid != null && levelFor(gid, null) < 999 ? `L${levelFor(gid, null)}` : null}
            selected={openGem?.lo === i && openGem?.slot === j}
            onclick={() => openGemPicker(i, j)}
            onclear={!locked && gid != null ? () => setGem(i, j, null) : undefined}
          >
            {#snippet tooltip()}
              {#if locked}
                <strong>Gem {j + 1} — locked</strong>
                <div class="tipsub">needs Mnemonic Retention rank {j - 7}</div>
                {#if gid != null}
                  <div class="tipsub warn">holds {nameFor(gid)} (imported) — raise the AA to use it</div>
                {/if}
              {:else if gid != null}
                {@render spellTip(gid)}
              {:else}
                <strong>Gem {j + 1}</strong>
                <div class="tipsub">empty — click to memorize a spell</div>
              {/if}
            {/snippet}
          </SlotWell>
          {#if j === BASE_GEMS - 1}
            <span class="gemdiv" title="gems 9–14 are granted by the Mnemonic Retention AA"></span>
          {/if}
        {/each}
      </div>
    </div>
  {/each}

  {#if loadouts.length === 0}
    <p class="setsempty">
      No spell sets yet — <em>Import…</em> your character's loadout INI, or add one below.
    </p>
  {/if}

  <div class="setsfoot">
    <button class="tb" onclick={addLoadout} disabled={loadouts.length >= MAX_SETS}>+ Add spell set</button>
    <span class="dim">{loadouts.length} / {MAX_SETS} sets</span>
  </div>
</section>

{#if openGem !== null}
  {@const g = openGem}
  {@const glo = loadouts[g.lo]}
  {@const cur = glo ? slotAt(glo, g.slot) : null}
  <div class="picker">
    <div class="pickhead">
      <span>
        <strong>{glo?.name ?? "set"}</strong> — gem <strong>{g.slot + 1}</strong>
        {#if cur != null} · {nameFor(cur)}{/if}
      </span>
      <span class="spacer"></span>
      {#if cur != null}<button class="mini danger" onclick={() => setGem(g.lo, g.slot, null)}>Clear</button>{/if}
      <button class="mini" onclick={() => (openGem = null)}>Close</button>
    </div>

    <div class="pickctl">
      <input class="picksearch" placeholder="search spells to memorize…" bind:value={gemSearch} />
      <label class="chk" title="scribed spells are what your character can actually memorize in game">
        <input type="checkbox" bind:checked={gemShowAll} />
        show all castable
      </label>
    </div>

    <ul class="picklist">
      <li class="grouphdr">Scribed in your book ({gemScribed.length})</li>
      {#each gemScribed as id (id)}
        {@const ic = iconFor(id)}
        <li>
          <button class="prow" onclick={() => pickGem(id)}>
            <span class="gem">
              {#if ic != null}{#key ic}<img src={spellIconUrl(ic)} alt="" draggable="false" onerror={hideOnError} />{/key}{/if}
            </span>
            <span class="pname">{nameFor(id)}</span>
            {#if tierOf(id) > 0}<span class="tag tier">+{tierOf(id)}</span>{/if}
            <span class="pcls">{castLine(id)}</span>
          </button>
        </li>
      {/each}
      {#if gemScribed.length === 0}
        <li class="none">nothing scribed matches — scribe spells in the book, or show all castable</li>
      {/if}

      {#if gemShowAll}
        <li class="grouphdr">Not scribed ({gemOther.length})</li>
        {#each gemOther as id (id)}
          {@const ic = iconFor(id)}
          <li>
            <button class="prow" onclick={() => pickGem(id)}>
              <span class="gem">
                {#if ic != null}{#key ic}<img src={spellIconUrl(ic)} alt="" draggable="false" onerror={hideOnError} />{/key}{/if}
              </span>
              <span class="pname">{nameFor(id)}</span>
              <span class="tag unscribed">not scribed</span>
              <span class="pcls">{castLine(id)}</span>
            </button>
          </li>
        {/each}
        {#if gemOther.length === 0}
          <li class="none">{loading ? "loading spells…" : "no other castable spells match"}</li>
        {/if}
      {/if}
    </ul>
  </div>
{/if}

<div class="bookframe">
  <div class="navcol">
    <button class="nav" onclick={() => (spread = 0)} disabled={spread <= 0} title="first pages">«</button>
    <button class="nav" onclick={() => (spread = Math.max(0, spread - 1))} disabled={spread <= 0} title="previous pages">‹</button>
  </div>

  <div class="book">
    <div class="page left">
      <div class="pagegrid">
        {#each leftPage(spread) as idx (idx)}{@render square(idx)}{/each}
      </div>
    </div>
    <div class="spine"></div>
    <div class="page right">
      <div class="pagegrid">
        {#each rightPage(spread) as idx (idx)}{@render square(idx)}{/each}
      </div>
    </div>
  </div>

  <div class="navcol">
    <button class="nav" onclick={() => (spread = Math.min(SPREADS - 1, spread + 1))} disabled={spread >= SPREADS - 1} title="next pages">›</button>
    <button class="nav" onclick={() => (spread = SPREADS - 1)} disabled={spread >= SPREADS - 1} title="last pages">»</button>
  </div>
</div>

<div class="pagenums">
  page {spread * 2 + 1} | {spread * 2 + 2} of {PAGES} &nbsp;·&nbsp; {SQUARES} squares
</div>

{#if openSquare !== null}
  {@const idx = openSquare}
  {@const scribed = scribedIdAt(idx)}
  <div class="picker">
    <div class="pickhead">
      <span>Square <strong>{idx + 1}</strong>{#if scribed != null} — {nameFor(scribed)}{/if}</span>
      <span class="spacer"></span>
      {#if scribed != null}<button class="mini danger" onclick={() => { unscribe(idx); }}>Clear</button>{/if}
      <button class="mini" onclick={() => (openSquare = null)}>Close</button>
    </div>

    {#if scribed != null}
      <div class="tierrow">
        <span class="tierlbl">Upgrade tier</span>
        <button class="mini" onclick={() => setTier(scribed, tierOf(scribed) - 1)} disabled={tierOf(scribed) <= 0}>−</button>
        <strong class="tiernum">+{tierOf(scribed)}</strong>
        <button class="mini" onclick={() => setTier(scribed, tierOf(scribed) + 1)} disabled={tierOf(scribed) >= 10}>+</button>
        {#if isSummon(scribed) && tierOf(scribed) > 0}
          <span class="summonnote">summon: +{tierOf(scribed)} pet level, scaled pet stats</span>
        {:else if isSummon(scribed)}
          <span class="summonnote dim">summon spell — raise tier to boost the pet</span>
        {/if}
      </div>
    {/if}

    <input class="picksearch" placeholder="search spells to scribe…" bind:value={pickSearch} />
    <ul class="picklist">
      {#each pickList as m (m.base.id)}
        {@const ic = iconFor(m.base.id)}
        <li>
          <button class="prow" onclick={() => { scribe(idx, m.base.id); openSquare = null; pickSearch = ""; }}>
            <span class="gem">
              {#if ic != null}{#key ic}<img src={spellIconUrl(ic)} alt="" draggable="false" onerror={hideOnError} />{/key}{/if}
            </span>
            <span class="pname">{m.base.name}</span>
            {#if m.base.is_summon}<span class="tag summon">summon</span>{/if}
            {#if m.base.is_song}<span class="tag song">song</span>{/if}
            <span class="pcls">{m.entries.map((e) => `${e.cls} ${e.level}`).join(" / ")}</span>
          </button>
        </li>
      {/each}
      {#if pickList.length === 0}
        <li class="none">{loading ? "loading spells…" : "no matching spells for this build"}</li>
      {/if}
    </ul>
  </div>
{/if}

<style>
  .toolbar { display: flex; gap: .5rem; align-items: center; margin-bottom: .4rem; flex-wrap: wrap; }
  .tb { background: #22262d; color: #cbb27a; border: 1px solid #8a7440; border-radius: 6px; padding: 4px 12px; cursor: pointer; font: inherit; }
  .tb:hover:not(:disabled) { color: #e8d9a8; border-color: #c9b26a; }
  .tb:disabled { opacity: .35; cursor: default; }
  .tb.cleardanger { background: #3a1a1a; color: #f88; border-color: #a33; }
  .tb.on { background: #141a24; color: #9cf; border-color: #46c; }
  .tb.sm { padding: 3px 9px; font-size: .78rem; }
  .tb.arm { background: #3a2a12; color: #f0b040; border-color: #b8791f; }
  .sep { width: 1px; height: 18px; background: #3a3f4a; }
  /* ---- write-to-game panel ---- */
  .writepanel { background: #12151c; border: 1px solid #2a2f38; border-radius: 8px; padding: .6rem .8rem; margin: 0 0 .7rem; max-width: 720px; }
  .wtitle { margin: 0 0 .3rem; color: #c9b26a; font-size: .84rem; }
  .wnote { margin: 0 0 .5rem; color: #8a94a6; font-size: .74rem; line-height: 1.5; }
  .wnote strong { color: #cbd; }
  .wnote code { background: #0e1117; border: 1px solid #2a2f38; border-radius: 4px; padding: 0 4px; }
  .wmuted { color: #778; font-size: .76rem; font-style: italic; }
  .wlist { list-style: none; margin: 0 0 .5rem; padding: 0; }
  .wlist li { display: flex; align-items: center; justify-content: space-between; gap: .6rem; padding: 3px 0; border-bottom: 1px solid #1c2029; }
  .wf { color: #cdd; font-size: .82rem; }
  .wf em { color: #778; font-style: normal; font-size: .74rem; }
  .sortsel {
    background: #1c1f26; color: #cbb27a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 3px 6px; font: inherit; font-size: .85rem; cursor: pointer;
  }
  .chk {
    display: flex; align-items: center; gap: 4px;
    color: #cbb27a; font-size: .8rem; cursor: pointer; user-select: none;
  }
  .chk input { accent-color: #c9b26a; cursor: pointer; margin: 0; }

  /* class chips read as toggle BUTTONS: gold fill + dark text ON, muted outline OFF */
  .clschip {
    background: transparent; color: #7d8496; border: 1px solid #4a5162; border-radius: 6px;
    padding: 2px 10px; cursor: pointer; font: inherit; font-size: .8rem; font-weight: 600;
    letter-spacing: .03em;
  }
  .clschip:hover { color: #cbb27a; border-color: #8a7440; }
  .clschip.on {
    background: linear-gradient(#e0c988, #c9b26a); color: #241811; border-color: #e8d9a8;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, .45), 0 0 0 1px rgba(201, 178, 106, .25);
  }
  .clschip.on:hover { color: #241811; border-color: #fff2c8; }
  .sccount { color: #667; font-size: .8rem; }

  /* ---- global tier override ---- */
  .tierbar { margin-bottom: .55rem; }
  .tierctl { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; }
  .tblabel { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .tslider { width: 150px; accent-color: #c9b26a; cursor: pointer; }
  .tval { color: #c9b26a; min-width: 2rem; text-align: center; font-size: .85rem; }
  .tnote { color: #8a7f66; font-size: .7rem; max-width: 520px; margin: .2rem 0 0; }
  .flash { color: #6c9; font-size: .78rem; }
  .note { color: #8a7f66; font-size: .74rem; max-width: 720px; margin: 0 0 .7rem; }
  .note em { color: #c9b26a; font-style: normal; }

  /* ---- spell sets = the memorized spell bar (the game's [SpellLoadouts]) ---- */
  .sets {
    max-width: 940px; margin: 0 auto .9rem; padding: .6rem .7rem;
    background: linear-gradient(#1a1712, #15130f);
    border: 1px solid #5a4326; border-radius: 8px;
    box-shadow: inset 0 0 0 1px rgba(201, 178, 106, .08);
  }
  .setshead { display: flex; gap: 1rem; align-items: flex-start; flex-wrap: wrap; margin-bottom: .55rem; }
  .settitle { flex: 1; min-width: 260px; }
  .settitle h3 {
    margin: 0; color: #e8d9a8; font-size: .95rem; font-variant: small-caps; letter-spacing: .06em;
  }
  .settitle h3 .dim { color: #8a7f66; font-size: .8rem; }
  .setsnote { color: #8a7f66; font-size: .72rem; margin: .15rem 0 0; max-width: 520px; }
  .dim { color: #8a7f66; font-size: .74rem; }

  .aa {
    display: flex; gap: .4rem; align-items: center; flex-wrap: wrap;
    padding: .35rem .55rem; background: #12151c; border: 1px solid #3a3f4a; border-radius: 6px;
  }
  .aalbl { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .aanum { color: #e8d9a8; min-width: 1.4rem; text-align: center; }
  .aahint { color: #667; font-size: .72rem; }
  .aahint strong { color: #c9b26a; }

  .setrow { padding: .45rem 0; border-top: 1px solid #2a2318; }
  .sethead { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin-bottom: .35rem; }
  .setname {
    width: 170px; padding: 3px 7px; font: inherit; font-size: .82rem;
    background: #1c1f26; color: #e8d9a8; border: 1px solid #8a7440; border-radius: 6px;
  }
  .setname:focus { outline: none; border-color: #c9b26a; }
  .setcount { color: #667; font-size: .74rem; }
  .setwarn { color: #c73; font-size: .74rem; }

  .gemrow { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  /* the base-8 / AA-granted split, visible at a glance */
  .gemdiv {
    width: 2px; height: 44px; margin: 0 5px;
    background: linear-gradient(#5a4326, #c9b26a, #5a4326);
    border-radius: 1px; opacity: .55;
  }

  .setsempty { color: #8a7f66; font-size: .76rem; margin: .3rem 0 .5rem; }
  .setsempty em { color: #c9b26a; font-style: normal; }
  .setsfoot { display: flex; gap: .6rem; align-items: center; margin-top: .5rem; }

  .tipsub { color: #89a; margin-top: 2px; }
  .tipsub.warn { color: #c73; }

  /* ---- open-book frame ---- */
  .bookframe { display: flex; align-items: stretch; gap: 8px; justify-content: center; }
  .book {
    display: flex; max-width: 720px; flex: 1;
    /* the spellbook emblem sits behind the pages as a faint watermark */
    background:
      url("/brand/spellbook-emblem.png") center/44% no-repeat,
      linear-gradient(#3a2a1a, #241811);
    border: 3px solid #1a120b; border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, .55), inset 0 0 0 2px #5a4326;
    padding: 12px;
  }
  /* keep the watermark subtle — it must never fight the squares */
  .book::before {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background: linear-gradient(#3a2a1a, #241811); opacity: .82; border-radius: 6px;
  }
  .book { position: relative; }
  .book > * { position: relative; z-index: 1; }
  .page {
    flex: 1; padding: 14px 14px;
    background: linear-gradient(#efe4c8, #e6d8b4);
    color: #3a2f1a;
  }
  .page.left {
    border-radius: 4px 0 0 4px;
    box-shadow: inset -14px 0 20px -14px rgba(60, 40, 15, .7);
  }
  .page.right {
    border-radius: 0 4px 4px 0;
    box-shadow: inset 14px 0 20px -14px rgba(60, 40, 15, .7);
  }
  .spine { width: 10px; background: linear-gradient(90deg, #b6a578, #7c6438, #b6a578); }
  .pagegrid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }

  .sqwrap { position: relative; }
  /* floating spell-info box under a hovered book square (dark, like item tooltips) */
  .stip {
    position: absolute; top: calc(100% + 5px); left: 50%; transform: translateX(-50%);
    min-width: 240px; max-width: 330px; z-index: 80;
    background: #0d0f14; border: 1px solid #8a7440; border-radius: 3px;
    padding: 6px 9px; font-size: .74rem; color: #bcd; text-align: left;
    box-shadow: 0 4px 14px rgba(0, 0, 0, .7);
    pointer-events: none;
  }
  .tipname { color: #c9b26a; font-weight: 600; margin-bottom: 2px; }
  .tiprow { color: #9ab; }
  .tiprow .up { color: #6c9; font-weight: 600; }
  .tiprow.dim { color: #667; }
  .approx { color: #667; margin-left: 3px; }
  .tipfocus { color: #d4f; font-size: .7rem; margin-top: 1px; }
  .tipapprox { color: #667; font-size: .64rem; font-style: italic; }
  .tipdesc { color: #89a; margin-top: 3px; border-top: 1px solid #262b33; padding-top: 3px; }

  .sq {
    position: relative; height: 62px; padding: 3px;
    background: rgba(255, 250, 235, .5);
    border: 1px solid #b09b6a; border-radius: 3px;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 2px; cursor: pointer; font: inherit; overflow: hidden;
  }
  .sq:hover { border-color: #7c6438; background: rgba(255, 250, 235, .8); }
  .sq.filled { background: rgba(255, 252, 240, .85); }
  .sq.sel { border-color: #7c6438; box-shadow: 0 0 0 2px #c9b26a; }
  .sq img { width: 30px; height: 30px; image-rendering: pixelated; }
  .sqname {
    font-size: .58rem; line-height: 1.05; color: #40331c; text-align: center;
    max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 0 2px;
  }
  .sqempty { font-size: .8rem; color: #b3a075; }
  .sqtier {
    position: absolute; top: 2px; right: 2px;
    background: #3a2f1a; color: #e8d9a8; border: 1px solid #8a7440; border-radius: 2px;
    font-size: .56rem; line-height: 1; padding: 1px 2px;
  }
  /* top-left: required cast level (display only), the tier's counterpart */
  .sqlvl {
    position: absolute; top: 2px; left: 2px;
    background: #241f14; color: #b8a97e; border: 1px solid #5a4d30; border-radius: 2px;
    font-size: .56rem; line-height: 1; padding: 1px 2px;
  }

  .navcol { display: flex; flex-direction: column; gap: 6px; }
  .nav {
    background: #241811; color: #c9b26a; border: 2px solid #5a4326; border-radius: 8px;
    font-size: 1.4rem; line-height: 1; padding: 0 10px; cursor: pointer; min-width: 40px; flex: 1;
  }
  .nav:hover:not(:disabled) { color: #e8d9a8; border-color: #c9b26a; }
  .nav:disabled { opacity: .3; cursor: default; }

  .pagenums { text-align: center; color: #8a7f66; font-size: .74rem; margin: .5rem 0 0; letter-spacing: .05em; }

  /* ---- scribe picker ---- */
  .picker {
    max-width: 720px; margin: .8rem auto 0; padding: .6rem;
    background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
  }
  .pickhead { display: flex; gap: .5rem; align-items: center; margin-bottom: .4rem; color: #9ab; font-size: .85rem; }
  .pickhead strong { color: #c9b26a; }
  .spacer { flex: 1; }
  .mini { background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px; padding: 1px 9px; cursor: pointer; font: inherit; font-size: .78rem; }
  .mini:hover:not(:disabled) { color: #e6e6e6; border-color: #2a6; }
  .mini:disabled { opacity: .35; cursor: default; }
  .mini.danger:hover { color: #f88; border-color: #a44; }

  .tierrow { display: flex; gap: .5rem; align-items: center; margin-bottom: .45rem; padding: .3rem .5rem; background: #12151c; border: 1px solid #3a3f4a; border-radius: 6px; }
  .tierlbl { color: #c9b26a; font-size: .75rem; font-variant: small-caps; letter-spacing: .08em; }
  .tiernum { color: #c9b26a; min-width: 2rem; text-align: center; }
  .summonnote { color: #6af; font-size: .72rem; margin-left: .3rem; }
  .summonnote.dim { color: #667; }

  .picksearch { width: 100%; box-sizing: border-box; padding: 6px 8px; background: #1c1f26; border: 1px solid #333; color: #e6e6e6; border-radius: 6px; margin-bottom: .4rem; }
  .pickctl { display: flex; gap: .6rem; align-items: center; margin-bottom: .4rem; }
  .pickctl .picksearch { flex: 1; margin-bottom: 0; }
  .picklist { list-style: none; padding: 0; margin: 0; max-height: 40vh; overflow: auto; }
  .picklist li { border-bottom: 1px solid #20242b; }
  .picklist li.none { color: #667; font-style: italic; padding: 4px; }
  .picklist li.grouphdr {
    color: #c9b26a; font-size: .7rem; font-variant: small-caps; letter-spacing: .08em;
    padding: 4px 4px 2px; border-bottom: 1px solid #3a3f4a; background: #12151c;
    position: sticky; top: 0; z-index: 1;
  }
  .prow { display: flex; gap: .5rem; align-items: center; width: 100%; text-align: left; background: none; border: none; color: #e6e6e6; padding: 3px 4px; cursor: pointer; font: inherit; }
  .prow:hover { background: #22262d; }
  .gem { width: 26px; height: 26px; flex-shrink: 0; background: #101318; border: 1px solid #2e3440; border-radius: 3px; display: flex; align-items: center; justify-content: center; }
  .gem img { width: 24px; height: 24px; image-rendering: pixelated; }
  .pname { flex-shrink: 0; }
  .tag { font-size: .62rem; border-radius: 4px; padding: 0 4px; }
  .tag.summon { background: #46c; color: #dee; }
  .tag.song { background: #2a6; color: #012; }
  .tag.tier { background: #3a2f1a; color: #e8d9a8; border: 1px solid #8a7440; }
  .tag.unscribed { background: #2a2024; color: #c66; border: 1px solid #a33; }
  .pcls { color: #89a; font-size: .74rem; margin-left: auto; }
</style>
