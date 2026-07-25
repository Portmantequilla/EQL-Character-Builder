// Typed mirror of the Tauri command contracts (serde snake_case fields;
// invoke args camelCase). Do NOT reshape these — they must match the Rust side.
import { invoke } from "@tauri-apps/api/core";

export interface Item {
  pageid: number; name: string; icon_id: number | null;
  slot: string | null; slots: string[]; classes: string[];
  races: string[]; deities: string[];
  ac: number | null; dmg: number | null; atk_delay: number | null; weapon_skill: string | null;
  haste_pct: number | null; required_level: number | null; recommended_level: number | null;
  stats: Record<string, number>; worn_effect: string | null; focus_effect: string | null;
  click_effect: string | null; era: string | null;
  flags: string | null;          // "Lore Equipped, Attunable" / "NO DROP" …
  weight: number | null; size: string | null;
  merchant_value: string | null; // "1pp, 1gp, 4sp, 3cp"
  non_canonical?: boolean;       // true = deliberately non-canonical; hidden by default
  is_epic?: boolean;             // class epic quest weapon; optimizer skips unless allowed
}

export interface SpellLoadout { name: string; slots: (number | null)[]; } // 14 gem slots; pageids or null

export interface BuildInput {
  name: string; level: number; classes: string[]; race: string | null;
  enabled_eras: string[]; // empty = all expansions enabled
  equipment: Record<string, number>;
  pet_equipment: Record<string, number>; // pet paperdoll keys "PET_HEAD", "PET_PRIMARY", … (legacy "PET_N" re-homed on load)
  equipment_tiers: Record<string, number>; // slot key -> 0..10; PLAYER and "PET_" slots share this map
  spell_tiers: Record<number, number>; // spell pageid -> 0..10
  spellbook: Record<number, number>; // book square index (0-based) -> wiki spell pageid
  loadouts: SpellLoadout[];
  /** LEGACY: standalone Mnemonic Retention rank 0..6. `aa_ranks` is the source of truth
   *  now (the engine takes the max of the two); kept so old saved builds still load. */
  aa_mnemonic_retention: number;
  aa_ranks: Record<number, number>; // AA planner: aa id -> purchased rank
  aa_points_available: number;      // points the character has to spend (user-entered)
  disabled_buffs: string[]; // buff NAMES toggled off
  strict_buffs: boolean;
  pet_summon_spell_id: number | null;
  pet_summon_tier: number; bard_in_group: boolean;
  /** manual pet inventory slot count; null = use the data-derived default */
  pet_slot_override: number | null;
  /** augment sockets: slot key (paperdoll or pet "PET_<SLOT>") -> socket type
   *  (ORNAMENTATION/FOCUS/CLICK/WORN/PROC) -> SOURCE item pageid */
  augments: Record<string, Record<string, number>>;
  /** active combat stance / invocation ids (display-only v1; one of each) */
  stance: string | null;
  invocation: string | null;
  /** spell pageids cast on you by OTHER PLAYERS (Spells-tab power-planner list);
   *  usable buff members with status EXTERNAL_CAST, capped by the buff slot cap */
  external_buffs: number[];
  /** spell ids manually picked for their lines (override the auto strongest; MANUAL badge) */
  manual_buffs: number[];
  /** item/consumable buff NAMES deliberately enabled (item sources never auto-apply) */
  other_buffs: string[];
  /** theorycrafting: skip the buff-cap trim (cap still displayed) */
  allow_over_cap: boolean;
  /** buff LINE names turned fully off (Clear All / eye) — no buff at all for the line */
  disabled_lines: string[];
}

export type MemberStatus =
  | "SELF_CAST" | "GROUP_BARD" | "ITEM"
  | "EXTERNAL_CAST" // cast on you by another player (usable — the external-buff list)
  | "EXTERNAL" | "UNKNOWN";

export interface ResolvedMember {
  name: string; status: MemberStatus; why: string; value: number | null;
  self_only: boolean; group: boolean; source_kind: string; spell_id: number | null;
  /** AUTO | MANUAL | OTHER | EXTERNAL | BARD | "" */
  source_tag: string;
  duration: string | null;
}

export interface LineResolution {
  line: string; statistic: string | null; effect_slot: number | null;
  bard_layer: number | null; chosen: ResolvedMember | null; n_usable: number; n_members: number;
  alternatives: ResolvedMember[]; rejected_reason: string | null;
}

export interface ActiveBuff {
  name: string; lines: string[]; total_value: number; status: MemberStatus; why: string;
  source_tag: string; // AUTO | MANUAL | OTHER | EXTERNAL | BARD
  duration: string | null;
}

export interface RejectedBuff { name: string; line: string; reason: string; }

export interface BuffPlan {
  lines: LineResolution[]; active: ActiveBuff[]; rejected: RejectedBuff[];
  buff_slots_used: number; buff_slot_cap: number;
}

export interface StatLine {
  /** flat stat bonuses from WORN item effects + WORN Exaltation augments (folded;
   *  effects also active as buffs are skipped — no double count) */
  item_effects: number;
  base: number; equipment: number; tier_bonus: number; buffs: number; raw_total: number;
  capped_total: number; over_cap: number; confidence: string;
}

export interface EquipWarning { slot: string; item: string; reason: string; status: string; }

export interface PetSummonInfo {
  spell_id: number; name: string; base_pet_level: number | null;
  pet_classes: string | null; pet_hp: number | null; pet_max_hit: number | null;
  class_levels: Record<string, number>;
  /** non-null = level/HP/hit partly from the research workbook's estimates
   *  (confidence label, e.g. "Estimated from legacy range") */
  estimate_confidence: string | null;
}

export interface PetGearSlot {
  slot: string; // pet paperdoll key "PET_HEAD", "PET_PRIMARY", …
  item_pageid: number | null; item_name: string | null; icon_id: number | null;
  badge: "EMPTY" | "FULLY_ACTIVE" | "PROC_INACTIVE" | "INVALID_CLASS" | "OUT_OF_ERA" | "OVER_CAP";
  reason: string | null;
  /** inside the class-combo slot budget (filled: counts; empty: can still take an item) */
  active: boolean;
}

/** A weapon/off-hand the pet was given, after the hand rule (1×2H / 2×1H / 1H+shield). */
export interface PetWeapon {
  slot: string; item_name: string;
  category: "2H" | "1H" | "SHIELD" | "OFFHAND";
  hand: "PRIMARY" | "SECONDARY" | null; // null = unwieldable under the rule
  active: boolean; note: string | null;
}

export interface PetBlock {
  summon: PetSummonInfo; valid: boolean; becomes_valid_at: number | null;
  calculated_level: number | null;
  intrinsic_classes: string[]; equip_class_pool: string[]; buff_lines: LineResolution[]; notes: string[];
  slot_count: number; default_slot_count: number; slot_count_overridden: boolean;
  slot_bonus_class: string | null;
  gear: PetGearSlot[]; gear_totals: Record<string, number>;
  weapon_config: PetWeapon[]; weapon_summary: string | null; weapon_warnings: string[];
  effective_tier: number; pet_hp_scaled: number | null; pet_max_hit_scaled: number | null;
  /** levels ACTUALLY gained above base after the player-1 cap (null = base unknown);
   *  only these grant +6% HP / +1 dmg / +5 skill points each (official rule) */
  levels_gained: number | null;
  skill_point_bonus: number;
  tier_capped: boolean; // some tier ranks were eaten by the cap (granted nothing)
}

/** One Alternate Advancement ability (wiki "Alternate Advancement" page). */
export interface AaAbility {
  id: number; name: string;
  category: "GENERAL" | "ARCHETYPE" | "CLASS" | "SPECIAL";
  class_abbr: string | null;    // CLASS rows only
  max_rank: number;
  costs: (number | null)[];     // per-rank; null = the wiki wrote "?" (unknown)
  cost_complete: boolean;       // false = the cost list has unknowns
  required_level: number | null;
  description: string;
}

/** What the AA planner reports back for the current build. */
export interface AaPlan {
  points_spent: number; points_available: number;
  cost_is_lower_bound: boolean; // a purchased rank has an unknown ("?") cost
  level_locked: string[];       // purchased AAs above the build's level (kept, flagged)
  class_locked: string[];       // purchased AAs the build's classes don't grant
}

export interface BuildCalculationResult {
  classes: string[]; level: number; race: string | null;
  wearable_item_count: number; stats: Record<string, StatLine>; equipment_haste_pct: number;
  buff_haste_pct: number; buff_plan: BuffPlan; pet: PetBlock | null;
  equipment_warnings: EquipWarning[];
  equipped_item_names: Record<number, string>;
  equipped_item_icons: Record<number, number>;
  spell_gem_count: number; // memorizable gems: 8 base + Mnemonic Retention rank (max 14)
  aa_plan: AaPlan;
  /** host slot key -> socketed Exaltation augments (validated; warnings advisory) */
  augment_grants: Record<string, AugmentGrant[]>;
  /** every effect worn gear + augments give the character (display-only for now) */
  effect_overview: ItemEffectOverview[];
  /** build classes locked at this level (no spells; third slot unlocks at 11) */
  locked_classes: string[];
  notes: string[];
}

/** One acquisition row for a spell (wiki spell_source). */
export interface SpellSourceRow {
  source_type: string; zone: string | null; npc: string | null;
  area: string | null; loc: string | null;
  class_source: string | null; // which class's guild vendor (CLASS_VENDOR rows)
  raw_text: string | null; // research components / verbatim wiki note
}
/** The `?` popup payload: mechanics, stacking lines, acquisition. */
export interface SpellInfo {
  id: number; name: string; description: string | null;
  mana: number | null; casting_time: number | null; recast_time: number | null;
  duration: string | null; target_type: string | null; resist_type: string | null;
  era: string | null; is_song: boolean;
  class_levels: [string, number, boolean][]; // (class, level, autogranted)
  buff_lines: string[];
  sources: SpellSourceRow[];
  item_sources: string[];
  wiki_url: string;
}

export interface SpellRow {
  id: number; name: string; class: string; required_class_level: number;
  is_autogranted: boolean; target_type: string | null; spell_type: string | null;
  is_beneficial: boolean; is_song: boolean; role: string | null; era: string | null;
  mana: number | null; duration: string | null;
  casting_time: number | null; recast_time: number | null; // seconds
  icon: string | null; is_summon: boolean;
  /** client-extracted ranges (eqlbuilds; PARTIALLY_VERIFIED — server can override) */
  dmg_min: number | null; dmg_max: number | null;
  heal_min: number | null; heal_max: number | null;
  resolved_description: string | null;
}

/** One combat mode (stance/invocation) — descriptions carry the real numbers. */
export interface Mode {
  id: string; kind: "stance" | "invocation"; name: string;
  message: string | null; description: string | null;
}
/** One merged skill line: BEST_OF cap across the build's classes. */
export interface SkillRow {
  name: string; trained_at: number | null; cap: number | null;
  best_class: string | null; classes: string[];
}

export interface FarmSource {
  item: string; mob: string; rarity: string | null; zone: string | null; mob_level: string | null;
}

export interface BuildSummary {
  id: number; name: string; level: number | null; classes: string[]; updated_at: string;
}

export interface StaticData {
  classes: string[]; races: string[]; paperdoll_slots: string[]; pet_summons: PetSummonInfo[];
  item_count: number; wiki_db_path: string;
  eras: string[]; // present in item data, unlock order
  default_enabled_eras: string[]; // live in-game now (wiki Template:PageEra)
  default_level_cap: number;      // live cap; slider may still exceed it
  exaltation_extract_min_tier: number; // +N gate for Exaltation extraction (Settings-editable)
  item_tier_scaling_pct: number;       // item upgrade %/tier — UI tierBonus must use this
  /** spell tier scaling (community-reconstructed 2026-07; Settings-editable) */
  spell_tier_scaling_pct: number;      // dmg/heal +%/tier, linear, floor
  spell_tier_mana_pct: number;         // mana -%/tier, PROVISIONAL
  spell_tier_mana_floor: number;       // base mana below this never shows reduction
  spell_tier_cast_pct: number;         // cast/recovery/reuse -%/tier
  reagent_conserve_pct_per_tier: number;
  /** starting-attribute tables (client-validated; eqltools.com Attributes) */
  race_base_stats: Record<string, Record<string, number>>;
  class_stat_mods: Record<string, Record<string, number>>;
  stat_naked_ceiling: number; // player-reported naked per-attribute ceiling
  stat_cap: number;           // buffed per-attribute hard cap (Stats tab Cap column)
  resist_cap: number;         // buffed resist/save hard cap (SV_* stats)
}

export interface AppInfo {
  name: string; version: string; author: string;
  org: string; copyright: string;
}

// ---- inventory import (/outputfile inventory dump) ----
/** One worn slot resolved to a wiki item. */
export interface MatchedSlot {
  slot: string; pageid: number; base_name: string; tier: number; game_name: string;
}
/** A worn item the wiki mirror doesn't have (kept, never silently dropped). */
export interface UnmatchedSlot {
  slot: string; game_name: string; base_name: string; tier: number; reason: string;
}
/** An Exaltation augment found in an inventory dump's socket sub-rows. */
export interface Exaltation {
  slot: string; socket: string; name: string;
  socket_type: string | null;    // FOCUS/CLICK/WORN/PROC/ORNAMENTATION (socket# mapping)
  source_pageid: number | null;  // the source item, resolved by name
}

// ---- Exaltation augments (socketed effects) ----
/** One catalog entry: an effect-bearing item as the augment it becomes at +4. */
export interface AugmentInfo {
  source_pageid: number;
  name: string;                 // "<source item> (Exaltation)"
  socket: string;               // FOCUS | CLICK | WORN | PROC
  effect_name: string;
  required_level: number | null;
  classes: string[];            // source item's class rule (empty/["ALL"] = none)
  races: string[];              // source item's race rule
  slots: string[];              // source item's wear slots (hand rule)
  flags: string | null;         // source item's restriction flags
  spell_id: number | null;      // linked effect spell (hover explanation)
  era: string | null;
  icon_id: number | null;
}
/** One socketed augment on an equipped item, validated (warnings are advisory). */
export interface AugmentGrant {
  slot: string; socket: string; source_pageid: number;
  name: string; effect_name: string; required_level: number | null;
  warnings: string[];
}
/** One effect worn gear (or a socketed augment) gives the character — display-only. */
export interface ItemEffectOverview {
  kind: string;              // PROC | WORN | FOCUS | CLICK
  label: string;             // "Combat Effect" | "Worn Effect" | …
  effect_name: string;
  source_slot: string;
  source_item: string;       // "Keg Mallet +5"
  via_augment: string | null;
  required_level: number | null;
  spell_id: number | null;   // for the hover explanation
  level_gated: boolean;
  warnings: string[];
}
/** What a linked effect spell does (hover explanation card). */
export interface SpellDetails {
  id: number; name: string; target_type: string | null;
  duration: string | null; mana: number | null;
  effects: string[];         // wiki's parsed lines, verbatim
}
/** What import_inventory returns. equipment/equipment_tiers/augments are ready to merge into a build. */
export interface InventoryImport {
  character: string | null;
  equipment: Record<string, number>;       // paperdoll slot -> wiki pageid (matched)
  equipment_tiers: Record<string, number>;  // paperdoll slot -> tier (matched, >0)
  augments: Record<string, Record<string, number>>; // slot -> socket type -> source pageid
  matched: MatchedSlot[];
  unmatched: UnmatchedSlot[];
  exaltations: Exaltation[];
  source_file: string;
}
/** One *-Inventory.txt the app auto-found in the EQL folder. */
export interface InventoryFile {
  path: string; name: string; character: string | null; modified_epoch: number;
}
/** The EQL folder the app scanned + the dumps in it (newest first). */
export interface InventoryScan { dir: string | null; files: InventoryFile[]; }

// ---- Loot Filter tab (AdvLoot LF_<Char>_<city>.ini editor) ----
/** One line of a loot-filter file. item_id is the GAME id (tier-independent, matches loot);
 *  filter_id is the disposition 2=Need / 3=Greed / 4=Never. base_name/tier/pageid enrich
 *  the read path and are ignored on write. */
export interface LfEntry {
  item_id: number; filter_id: number; icon_id: number; name: string;
  base_name: string; tier: number; pageid: number | null;
}
export interface LfFile {
  path: string; name: string; character: string | null; city: string | null;
  entry_count: number; modified_epoch: number;
}
export interface LfScan { dir: string | null; files: LfFile[]; }
export interface LfDoc {
  path: string; character: string | null; city: string | null; entries: LfEntry[];
}
/** A picker row that carries a REAL game id (from the harvested catalog). */
export interface CatalogItem {
  game_item_id: number; name: string; icon_id: number | null; pageid: number | null;
}
/** A wiki item on the "all items" side; game_item_id is set only when the catalog knows one. */
export interface WikiPick {
  pageid: number; name: string; icon_id: number | null; slot: string | null;
  game_item_id: number | null;
}

/** An editable engine rule (formula_table). `verification_status` is one of
 *  WIKI_CONFIRMED | PARTIALLY_VERIFIED | NEEDS_INGAME_TEST | MANUAL_OVERRIDE
 *  | LEGACY_EQ_DATA | VERIFIED_INGAME. */
export interface FormulaRow {
  formula_key: string; value: string; description: string | null;
  verification_status: string;
  source: string | null; is_user_edited: boolean;
}

export const appInfo = () => invoke<AppInfo>("app_info");
export const openDataFolder = () => invoke<void>("open_data_folder");

export const getStatic = () =>
  invoke<StaticData>("get_static");
export const resolveBuild = (build: BuildInput) =>
  invoke<BuildCalculationResult>("resolve_build", { build });
export const queryItems = (classes: string[]) =>
  invoke<Item[]>("query_items", { classes });
export const querySpells = (classes: string[], level: number) =>
  invoke<SpellRow[]>("query_spells", { classes, level });
export const listAas = () =>
  invoke<AaAbility[]>("list_aas"); // every AA the wiki publishes (all categories)
export const listAugments = () =>
  invoke<AugmentInfo[]>("list_augments"); // the Exaltation augment catalog (item-edit popup)
export const spellDetails = (ids: number[]) =>
  invoke<Record<number, SpellDetails>>("spell_details", { ids }); // effect hover cards
export const listModes = () =>
  invoke<Mode[]>("list_modes"); // stances + invocations (eqlbuilds snapshot)
export const querySkills = (classes: string[]) =>
  invoke<SkillRow[]>("query_skills", { classes }); // BEST_OF merged skill lines
export const externalReceivable = () =>
  invoke<number[]>("external_receivable"); // spell ids receivable as external buffs
export const spellInfo = (id: number) =>
  invoke<SpellInfo>("spell_info", { id }); // the `?` popup payload
export interface OtherBuffRow {
  name: string; line: string; source_kind: string;
  value: number | null; source_items: string | null;
}
export const listOtherBuffs = () =>
  invoke<OtherBuffRow[]>("list_other_buffs"); // Add Other Buff catalog
/** One FOCUS-bearing item + its drop sources (Focus Effects reference tab). */
export interface FocusEffectRow {
  effect_name: string; description: string | null;
  item_pageid: number; item_name: string; item_classes: string[];
  era: string | null;
  sources: [string, string | null][]; // (mob, zone)
}
export const focusEffects = () =>
  invoke<FocusEffectRow[]>("focus_effects");
/** One Exaltation-extractable item (FOCUS/CLICK/WORN/PROC; regen excluded) + its
 *  drop sources (Exaltations reference tab). */
export interface ExaltationRow {
  kind: "FOCUS" | "CLICK" | "WORN" | "PROC";
  effect_name: string; description: string | null;
  required_level: number | null;
  item_pageid: number; item_name: string; item_classes: string[];
  era: string | null;
  sources: [string, string | null][]; // (mob, zone)
  /** the effect spell's parsed mechanical lines ("Increases your faction with … by 250") */
  effect_lines: string[];
}
export const exaltationEffects = () =>
  invoke<ExaltationRow[]>("exaltation_effects");
/** One structured spell_effect row of a worn FOCUS spell (Spellbook focus math). */
export interface FocusDetailRow {
  spell_id: number; opcode: string;
  base_amount: number | null; max_amount: number | null; raw_text: string;
}
export const focusDetails = (ids: number[]) =>
  invoke<FocusDetailRow[]>("focus_details", { ids });
/** Structured focus effect decoded from the client (exact limits) for Spellbook math. */
export interface FocusClient {
  spell_id: number;
  kind: "DMG" | "HEAL" | "HASTE" | "MANA" | "DURATION" | "RANGE" | "REAGENT";
  pct_min: number; pct_max: number;
  max_level: number | null; level_decay_pct: number | null;
  min_level: number | null; min_duration_ticks: number | null;
  beneficial_only: boolean; detrimental_only: boolean;
}
export const focusClient = (ids: number[]) =>
  invoke<FocusClient[]>("focus_client", { ids });
export const openUrl = (url: string) =>
  invoke<void>("open_url", { url }); // system browser (wiki links)
export const farmList = (itemNames: string[]) =>
  invoke<FarmSource[]>("farm_list", { itemNames });
export const chooseForMe = (seed: number, level: number, classes: string[], enabledEras: string[]) =>
  invoke<BuildInput>("choose_for_me", { seed, level, classes, enabledEras });
/** One-click gear optimization. profile: "OPTIMAL" (survival) | "MINMAX" (max offense).
 *  Returns the build with worn player gear + Exaltations replaced (pet gear kept).
 *  allowEpic: include the class epic quest weapons in suggestions (off = drops only). */
export const optimizeGear = (build: BuildInput, profile: "OPTIMAL" | "MINMAX", allowEpic = false) =>
  invoke<BuildInput>("optimize_gear", { build, profile, allowEpic });
/** Suggest PET gear (fills only the pet's active-slot budget with its class's best items). */
export const optimizePetGear = (build: BuildInput, profile: "OPTIMAL" | "MINMAX") =>
  invoke<BuildInput>("optimize_pet_gear", { build, profile });
export const saveBuild = (build: BuildInput) =>
  invoke<number>("save_build", { build });
export const listBuilds = () =>
  invoke<BuildSummary[]>("list_builds");
export const loadBuild = (id: number) =>
  invoke<BuildInput>("load_build", { id });
export const deleteBuild = (id: number) =>
  invoke<void>("delete_build", { id });
export const spellIcons = (ids: number[]) =>
  invoke<Record<number, string>>("spell_icons", { ids });
export const spellLines = () =>
  invoke<Record<number, string>>("spell_lines"); // pageid -> buff line/family name (buff spells only)
export const exportSpellbook = (buildName: string, loadouts: SpellLoadout[]) =>
  invoke<string>("export_spellbook", { buildName, loadouts });
export const importSpellbook = (path: string) =>
  invoke<SpellLoadout[]>("import_spellbook", { path });

/** A real `<Char>_<city>_LO1.ini` settings file we can safely merge spell sets into. */
export interface LoadoutFile {
  path: string; name: string; character: string | null; city: string | null;
  set_count: number; modified_epoch: number;
}
/** Result of a merge-write: the file, its backup, sets written, and gems that couldn't map. */
export interface LoadoutWrite {
  path: string; backup: string | null; sets_written: number; slots_unresolved: number;
}
export const listLoadoutFiles = () =>
  invoke<LoadoutFile[]>("list_loadout_files"); // real <Char>_<city>_LO1.ini in the EQL folder
export const exportSpellbookToGame = (path: string, loadouts: SpellLoadout[]) =>
  invoke<LoadoutWrite>("export_spellbook_to_game", { path, loadouts }); // safe in-place merge

// ---- macros (socials): [Socials] section of <Char>_<city>_LO1.ini ----
/** A social macro: a button at page/button with a name, chat-color 0-15, and up to 5 lines. */
export interface Social {
  page: number; button: number; name: string; color: number; lines: string[];
}
export interface SocialWrite { path: string; backup: string | null; count: number; }
export const readSocials = (path: string) =>
  invoke<Social[]>("read_socials", { path });
export const writeSocials = (path: string, socials: Social[]) =>
  invoke<SocialWrite>("write_socials", { path, socials }); // replaces [Socials], preserves the rest
export const exportSocialsDesktop = (label: string, socials: Social[]) =>
  invoke<string>("export_socials_desktop", { label, socials }); // shareable fragment to Desktop

// ---- inventory: import worn gear (with +N tiers) from a /outputfile inventory dump ----
export const importInventory = (path: string) =>
  invoke<InventoryImport>("import_inventory", { path });
export const listInventoryFiles = () =>
  invoke<InventoryScan>("list_inventory_files"); // auto-found dumps in the EQL folder
export const setEqlDir = (path: string) =>
  invoke<InventoryScan>("set_eql_dir", { path }); // point at the game folder, re-scan

// ---- loot filter: read/write LF_<Char>_<city>.ini + the harvested game-id catalog ----
export const lfListFiles = () =>
  invoke<LfScan>("lf_list_files");
export const lfRead = (path: string) =>
  invoke<LfDoc>("lf_read", { path }); // also harvests the file's game ids into the catalog
export const lfWrite = (character: string, city: string, entries: LfEntry[]) =>
  invoke<string>("lf_write", { character, city, entries }); // returns the written path
export const lfImportInventory = (path: string) =>
  invoke<number>("lf_import_inventory", { path }); // harvest ids -> catalog, returns rows
export const lfCatalogSearch = (query: string, limit?: number) =>
  invoke<CatalogItem[]>("lf_catalog_search", { query, limit });
export const lfCatalogCount = () =>
  invoke<number>("lf_catalog_count");
export const lfWikiSearch = (query: string, limit?: number) =>
  invoke<WikiPick[]>("lf_wiki_search", { query, limit });

// ---- settings: the editable game rules the engine reads ----
export const listFormulas = () =>
  invoke<FormulaRow[]>("list_formulas");
// set_formula also refreshes the engine snapshot on the Rust side, so the next
// resolve_build already uses the new value.
export const setFormula = (key: string, value: string, verifiedIngame: boolean) =>
  invoke<void>("set_formula", { key, value, verifiedIngame });

// ---- build sharing: Desktop/EQLBuilder Exports/<name>.eqlbuild.json ----
export const exportBuild = (build: BuildInput) =>
  invoke<string>("export_build", { build }); // returns the written path
export const importBuild = (path: string) =>
  invoke<BuildInput>("import_build", { path });
