# EQL Character Builder - Full Plan (v2)

Synthesized 2026-07-12 from: user spec (`spec.md` §1-22), four recon reports
(current-state, spells, pets, character-system), and four design documents (schema,
importer, engine, app), with every finding from two adversarial review passes applied.
**This document supersedes all eight inputs. Where an input contradicts this document,
this document wins.**

Canonical decisions made here (the review found the design docs contradicted each other
on all five; each is argued in its section):

1. **Engine = pure Rust crate `eql-engine`** behind Tauri IPC. The webview contains zero
   game rules. (§4.1)
2. **Two databases**: disposable `wiki.db` (rebuilt + atomically swapped on sync) and
   precious `builds.db`, both in `%LOCALAPPDATA%/EQLBuilder/` — never OneDrive. The DDL
   in §2 is THE canonical schema; the importer and app both execute it from one generated
   `docs/schema.sql`. (§2)
3. **One verification vocabulary** (six values) used by schema CHECKs, importer stamps,
   engine `FormulaConfidence`, and UI badges. (§2.0.2)
4. **One formula store**: the dimensioned `formula_table` in builds.db (engine design),
   replacing the schema doc's `formula(key,value_json)` and the importer doc's
   `formula_table(name,key,value)` + `game_rule`. (§2.2, §4.7)
5. **Build rows never hard-FK wiki rows.** Builds store `pageid` + `name_canonical` soft
   references, re-resolved at snapshot load; unresolvable selections become
   `SAVED_INACTIVE (DATA_MISSING)`, never deleted. (§2.2.0)

Naming law (spec §2, restated): the column is **`required_class_level`** (when a class
learns a spell) and **`spell_upgrade_tier`** (tier 0-10). The spec's own §2 SQL snippet
uses `required_level`; that snippet is superseded by the spec's naming law and by this
document. Never a bare `spell_level` anywhere.

---

## 0. Executive summary

The goal is a Tauri desktop app for planning a triple-class EverQuest Legends character:
equipment with +0..+10 tiers, Exaltation sockets, spells with 0..10 upgrade tiers, a full
buff-stacking engine (player and pet resolved separately), pet summoning/validation, a
farm list, and a seeded "Choose for me" randomizer.

Pipeline shape: **Python importer** (extends the proven `eql_wiki_sync.py`) mirrors the
wiki into `wiki.db`; curated **YAML overrides** patch wiki gaps and are re-applied on
every sync; a **Rust engine crate** loads an immutable in-memory snapshot and computes
the spec-§12 `build_calculation_result` deterministically; a **Svelte webview** renders
that one result object on every page. Everything the wiki does not state ships as an
editable, badged formula-table row — never a hidden constant — with an in-game
verification checklist (§8) to burn the unknowns down during beta.

### Gaps filled (user's original spec → this plan)

| # | Gap in the original spec | Resolution |
|---|---|---|
| 1 | No core spell tables defined (`spell`, `spell_class_level`, effects) — the whole spell domain was absent from the data pipeline | §2.1 Group 3 full DDL; §3 spell importer for `{{Spellpage}}`/`{{Spellpagesmart}}` (1,939 + 647 pages) |
| 2 | No races / deities / classes / base-stats / skills reference data | §2.1 Group 2 tables + seeds; `sync-static` importer groups (§3.2) |
| 3 | Item required/recommended level never parsed (exists as free text on 19 items; 738 items carry per-effect `at Level N`) | `items.required_level/recommended_level` columns + `item_effect.required_level`; `reparse-items` pass (§3.4) |
| 4 | Item `Deity:` line (378 items) dropped by v1 parser | `item_deity` join table (multi-deity capable) (§2.1 Group 5) |
| 5 | Exaltations: zero data anywhere, no wiki entity pages | Derived `exaltation` rows from harvested `item_effect`s + seeded socket-unlock-tier table; natural key so socket picks survive re-imports (§2.1 Group 5) |
| 6 | Spell upgrade tier (0..10) scaling — zero wiki presence | `formula_table` key `spell_tier_scaling` (default mirrors item rule), NEEDS_INGAME_TEST, editable (§4.7) |
| 7 | Multi-class stat-combination math absent from wiki | `formula_table` keys `class_attr_combine` (default SUM), `multi_class_hp_combine` / `..mana..` / `..skill..` (default BEST_OF); assumption banner (§4.7, §5) |
| 8 | Base HP/mana curves: wiki has only L50/L60 anchors | `formula_table` dimensioned curves `base_hp(class,level)`, `hp_per_sta(class,level)`, `mana_per_stat(class,level)` (§4.7) |
| 9 | MAG/ENC/SHM base pet levels absent from wiki | `spell_pet_summon.base_pet_level` NULL + seeded override stubs; engine models "level unknown" explicitly (§4.6.1); backlog V1 |
| 10 | Farm list had no data model | `mob_zone` + mob level normalizer + `build_wishlist` + farm views (§7) |
| 11 | No storage/coexistence story for wiki vs build data | Two-database architecture with soft references + reconciliation (§2, §5.2) |
| 12 | "Choose for me" undesigned | Full seeded PCG32 algorithm with budget tiers + repair (§6) |
| 13 | Buff Lines page incomplete + pet buff lines missing entirely | Buff Lines parsed as `verified=0` seed; shipped `overrides/seeds/pet_buff_lines.yaml` defines PET_HASTE / PET_AC / PET_STRENGTH lines (§3.6, §4.5) |
| 14 | Spells-page filters Damage / Control / Utility have no wiki backing | APP_DEFINED `spell.role` heuristic computed at import from parsed effect opcodes (§3.3) |
| 15 | Per-spell tier selection had no home outside buff profiles | `build_spell_tier` table + explicit precedence rule (§2.2) |
| 16 | Whether ordinary Single/Group buffs can land on a pet — unstated | `spell_target_rule.pet_targetable` with documented default derivation, NEEDS_INGAME_TEST (§2.1, backlog V4) |
| 17 | Item-level requirement vs pets (CAN_USE_STATS) undefined | Explicit rule in §4.6.2, NEEDS_INGAME_TEST (backlog V5); drives the INVALID_PET_LEVEL badge |
| 18 | Instrument resonance→multiplier curve unknown (one anchor: +28 = 280%) | Editable 29-row lookup seeded linearly; calibration checkpoints from Buff Lines dual values (§4.4) |
| 19 | AC softcap / resist conversion only as P99/EQEmu legacy data | `formula_table` rows tagged LEGACY_EQ_DATA, editable (§4.7) |
| 20 | AA / Stances & Invocations / Rituals exist in Legends but were not in the spec | Declared out of scope with UI note + reserved extension points (§5.6) |
| 21 | Data/formula versioning had no writers | Post-swap hook writes `data_version`; formula editor writes `formula_version` (§2.2, §5.2) |
| 22 | Wiki page deletions/renames/dedup could orphan saved builds | pageid + name_canonical soft refs, re-resolution, DATA_MISSING status (§2.2.0) |

---

## 1. Data reality check — what eqlwiki.com actually provides

Sources: recon probes of 2026-07-12 (34 spell pages, Pet Guide, Buff Lines, Statistics,
Game Mechanics, Item Upgrade System, Exaltations, Skills, zone pages; raw caches under
`scratchpad/probe-*`), plus read-only audit of `db/eql.db` (items 10,758 · mobs 6,340 ·
drops 32,138).

### 1.1 Spell pages

- Two templates, one field set: `{{Spellpage}}` and `{{Spellpagesmart}}` (newer, "under
  construction", adds class-page transclusion plumbing). Core fields 100% consistent
  across the 33-page sample: `spellname, spellicon, description, classes, slots, skill,
  mana, range, casting_time, fizzle_time, recast_time, duration, target_type, spell_type,
  resist, msg_*, where_to_obtain, items_with_effect, other`.
- Per-class acquisition: `classes` bullets `* [[Druid]] - Level 10 (Autogranted)` —
  this is `spell_class_level` for free.
- Effects: `{{SpellSlotRow|<slot#>|<text>}}` with REAL sparse game slot numbers
  (Burnout uses slots 3+4 only) and a parseable grammar including caster-level endpoints
  `Increase STR by 16 (L18) to 18 (L22)`.
- Structured stacking rules exist as slot rows:
  `Stacking: Block new spell if slot 3 is effect 'Max Hitpoints' and < 1100` (Aegolism).
- Prose stacking in descriptions: `Does NOT stack with [[X]]`, `replacing [[X]], [[Y]]`.
- Bard songs: BRD class + instrument `skill` + mana 0 + pseudo-slot
  `Enhanced by instrument? Yes/Required`.
- Duplicates: live case-variant pages exist (`Anthem de Arms` vs `Anthem De Arms`) —
  dedup required.
- Era tags on only ~40% of pages.

### 1.2 Buff Lines page (pageid 50578, ~1,790 lines)

The stacking bible: per-statistic sections, sub-lines literally named by effect slot
(`AC (Slot 2)`, `AC (Layer 2, Slot 1)` = bard layer), descending-strength bullets
`* +52 [[Deliriously Nimble]] ([[Shaman]] 53)` with `Click:/Proc:/Worn:/Consumable:`
annotations and bard dual values `+35 (+98)`. Combination-buff semantics stated in prose.
~90% of bullets regex-parseable. **Self-declared incomplete** (missing item buffs, "+0
rows"); values are a level-60-caster snapshot. Documented facts: 15-buff limit including
bard songs (stated twice); all bard effects except haste & mana regen instrument-scaled;
max instrument bonus +28 = 280%. **All Buff Lines sections are player statistics — there
are no pet buff-line sections**; pet lines must be shipped as seed overrides.

### 1.3 Pets (Pet Guide pageid 50581 + spell pages + `{{Summonedpetpage}}`)

DOCUMENTED (quoteable): all 8 intrinsic class pairs (matching spec §13, plus SHD=WAR/SHD);
rank rule "Each rank of pet spells increase the summoned pet's level by one, capped one
level under the summoner"; +6% HP / +1 dmg / +5 skill points per upgraded level; equipment
rule = pet classes UNION owner classes (with worked example); "Pets will ALWAYS respect
proc level requirements"; pet inventory slot counts (base 4; +3 MAG/BST, +2 NEC,
+1 ENC/DRU/SHM, +0 SHD); armor slot AC-priority rule; `target_type=Pet` marker (Burnout);
NEC base pet levels (spell-page `other` blocks + `skel_pet_N_` tokens); BST base levels +
full statblocks (`{{Summonedpetpage}}`, canonical "X Summon" pages, `{{Delete}}`-tagged
duplicates skipped).

PARTIAL: dual-wield unlock level (BST=5, all 8 other families literally "?"); non-BST
innate abilities (prose only); legacy per-family stat tables explicitly stale.

ABSENT (in-game verification needed): base pet levels for MAG/ENC/SHM (tokens
`SumEarthR4`, `Animation6`, `SpiritWolf227` carry no level); deity restrictions on pet
items ("Needs testing in EQ Legends" on the page itself); class-restricted transferred
proc/Exaltation activation on pets; EQL pet stat scaling.

### 1.4 Character system

Races: full 15-race base-stat table (Kerra row "assumed from Vah Shir" → unverified) +
per-class additive stat modifiers on `Statistics`. Race restricts the **Primary class
only**; combination math for three classes is nowhere stated. Deities: 16 + Agnostic;
item restriction via `Deity:` statsblock line. Stat caps: hard 255, soft 200 WIS/INT/CHA.
HP: STA→HP per class at L50/L60 only. Mana: anecdotal ~11/pt at L60. AC softcap and
resist conversion are P99/EQEmu-derived with explicit "may differ in EQL" flags. Skills:
master list + max caps on ~85 `Skill X` pages; per-level curves fragmentary. L51+ buffs
have documented target-level floors (≤52 lands on 40+ … 45+). Item Upgrade System page:
full tier 0-10 math DOCUMENTED (cumulative +10%/tier, round down, min +1, weapon dmg
+10%, delay never reduced, 2^n XP ladder, motes). Exaltations page: socket unlock tiers
Orn+0/Focus+1/Click+2/Worn+3/Proc+4, harvest/transfer, class-intersection shrink, slot
inheritance — but note the internal "+4 fully upgraded" vs "+10 tiers" inconsistency.
Adjacent Legends systems the spec ignores: **Alternate Advancement, Stances &
Invocations, Rituals** — declared out of scope in §5.6.

### 1.5 Spec §5 verdict table (buff-specific data)

| Data point | Verdict | Source / fallback |
|---|---|---|
| spell ID | PRESENT | MediaWiki pageid (+revid provenance) |
| class + required class level | PRESENT | `classes` bullets |
| era | PARTIAL (~40%) | `{{X Era}}` tag → default Classic + override; `era_source` recorded |
| source: autogranted / vendor / drop | PRESENT | class bullets / `SpellWhereRow` / drop bullets |
| source: quest/research | ABSENT-ish | free text → `UNKNOWN` + override |
| spell upgrade tier support | **ABSENT** | override + in-game testing (backlog V6) |
| target Self/Single/Group/Party/Pet | PRESENT | `target_type` (`Group v2`, `Party` → GROUP) |
| pet subtypes (summoned/charmed/owner-only) | ABSENT | override-only (`pet_subtype`) |
| Corpse target | assumed | verify during full import |
| target level min | ABSENT | override-only; global L51+ floor table is a formula |
| target level max | DERIVABLE | `SingleL65` suffix |
| target exclusions | ABSENT | override-only |
| duration classes | DERIVABLE | parse free text; `UNKNOWN` fallback |
| bard pulse | DERIVABLE | class+skill+ticks+mana0 |
| proc/click-triggered | DERIVABLE | `items_with_effect` + item effect lines |
| discipline / aura / maintained toggle | ABSENT | reserved enum values |
| duration formula | PARTIAL | endpoints present; interpolation NEEDS_INGAME_TEST |
| tick count / recast | PRESENT | explicit or seconds/6 |
| effect slot number | PRESENT | `SpellSlotRow` first arg |
| effect opcode / statistic | DERIVABLE | normalize effect-text verbs |
| base/max amount + caster-level scaling | PRESENT | `by A (Lx) to B (Ly)` |
| spell-tier scaling | ABSENT | formula fallback |
| instrument scaling | PARTIAL | boolean present; multiplier curve NEEDS_INGAME_TEST |
| current vs max resource | PRESENT | distinct wordings |
| cosmetic only | DERIVABLE | illusion-only slot rows |
| adds a proc | expected | `Add Proc:` wording, parse-TODO |
| buff line / sub-line | PARTIAL | 6 `X line` categories + Buff Lines page seed |
| explicit block + thresholds | PRESENT | structured `Stacking:` rows |
| explicit overwrite | expected | same grammar, zero samples — parser accepts both verbs |
| known (in)compatible spells | PARTIAL | prose scan + curation |
| combination-buff relationships | PARTIAL | Buff Lines columns + prose; curated |
| illusion category | PRESENT | `Category:Illusions` |
| haste/regen category | DERIVABLE | effect text + Buff Lines sections |
| confidence + source revision | PRESENT | revid per page; Buff Lines defaults verified=0 |

### 1.6 Spec §20 verdict table (importer additions) — 24 bullets

PRESENT 12 (beneficial flag, class+level, effect slots, block rules, target type,
pet-target status, caster scaling, min/max values, duration endpoints, instrument type,
summon token, pet class pair) · DERIVABLE 6 (overwrite rules grammar, bard song status,
proc/click required level, exaltation effect level, effect class restrictions, stacking
notes via prose scan) · SEED+OVERRIDE 5 (buff-line membership, instrument scaling
multiplier, pet base level, pet innate abilities, target-level restrictions) ·
OVERRIDE-ONLY components: duration interpolation, instrument multiplier curve,
MAG/ENC/SHM base levels, target-level minimums. Fully wiki-ABSENT beyond §20: spell tiers
0-10 + scaling, pet-target subtypes, multiclass combination math, XP curve, per-level
skill-cap curves, **pet buff lines** (shipped as seed overrides).

### 1.7 Current pipeline debt this plan pays down

v1 `eql_wiki_sync.py` silently drops every non-item/mob page (spell pages fetched by
`--incremental` are discarded); child tables use `INSERT OR REPLACE` without pre-delete
(stale rows); `Required level of N.` / `Deity:` / `Resonance:` / `at Level N` lines
unparsed; `item_stats` carries typo keys (`SV POISION`); `item_races` has dirty tokens
(`ALL<BR>`, `except`, `None`); `item_classes` has `ALL`/`NON`; `mobs.level` free text;
`mobs.zone` comma-joined; one `Template:NPC` page ingested as a mob. All addressed in §3.

---

## 2. Database schema v2 — canonical DDL

**This section is the single source of truth for every table.** A generated
`docs/schema.sql` (two sections, one per database) is executed by the importer
(wiki.db section, M0) and by the app's migration runner (builds.db section, M1).
The importer document's parallel DDL is deleted; §3.9 gives the importer's
column-mapping onto these tables.

### 2.0 Schema-wide policies

**2.0.1 Topology.** Two SQLite files under `%LOCALAPPDATA%/EQLBuilder/`:

```text
%LOCALAPPDATA%/EQLBuilder/
  data/wiki.db      disposable; rebuilt as wiki.db.building and ATOMICALLY RENAMED on sync
  data/raw/         raw wikitext cache (same scheme as the project raw/)
  builds.db         precious user data; app-migrated; export/import in Settings
  overrides/*.yaml  curated corrections (spec §20 format); compiled INTO wiki.db at import
```

The OneDrive `db/eql.db` remains the research/dev mirror only; **the app never opens a
SQLite handle inside OneDrive** (documented corruption hazard). First run seeds wiki.db
by copying the mirror or re-parsing `raw/`.

**No FK ever crosses the file boundary.** builds.db columns that reference wiki entities
(`item_id`, `spell_id`, `race_id`, `deity_id`, `class_id`, archetype ids, zone ids) are
plain INTEGERs; spells and items additionally carry a `*_name_canonical` companion for
reconciliation (2.0.4). `class`/`race`/`deity`/`pet_archetype` ids are **seeded stable
constants** (defined in 2.1 Group 12) identical across rebuilds, so plain-id references
to them are safe. Views that need both files are created as TEMP views on the app's
connection after `ATTACH DATABASE 'wiki.db' AS wiki` (read-only).

**2.0.2 Verification vocabulary — one enum everywhere.**

```text
verification_status ∈ { WIKI_CONFIRMED, PARTIALLY_VERIFIED, NEEDS_INGAME_TEST,
                        MANUAL_OVERRIDE, LEGACY_EQ_DATA, VERIFIED_INGAME }
```

Confidence rank (best→worst) for UI badges and engine tie-breakers:
`VERIFIED_INGAME > WIKI_CONFIRMED > MANUAL_OVERRIDE > PARTIALLY_VERIFIED >
NEEDS_INGAME_TEST > LEGACY_EQ_DATA`.

Translation table (binding on the importer, engine, and UI):

| Old term (importer/engine/app drafts) | Canonical value |
|---|---|
| DOCUMENTED / Documented / VERIFIED | WIKI_CONFIRMED |
| PARTIALLY_VERIFIED / PartiallyVerified | PARTIALLY_VERIFIED |
| SEED_UNVERIFIED / UNVERIFIED / DERIVED / Unverified | NEEDS_INGAME_TEST |
| OVERRIDE / UserOverride | MANUAL_OVERRIDE |
| VERIFIED_IN_GAME (`tested.method: in_game`) | VERIFIED_INGAME |
| LEGACY_EQ_DATA / LegacyEqData / LEGACY | LEGACY_EQ_DATA |

The engine enum is `FormulaConfidence { WikiConfirmed, PartiallyVerified,
NeedsIngameTest, ManualOverride, LegacyEqData, VerifiedIngame }` — a 1:1 mirror.
UI badges (§5.5): no glyph for VERIFIED_INGAME/WIKI_CONFIRMED · ◆ MANUAL_OVERRIDE ·
◐ PARTIALLY_VERIFIED · ○ NEEDS_INGAME_TEST · ◇ LEGACY_EQ_DATA.

**2.0.3 Enum policy.** Every closed value list is a TEXT column with an inline
`CHECK (x IN (...))`, UPPER_SNAKE, spec value lists verbatim. Lookup tables only for real
entities (`class`, `race`, `deity`, `skill`, `buff_line`, `zone`, `pet_archetype`).
Exception: `spell_effect.opcode` is open-vocabulary TEXT (importer-extensible grammar).
Values not in the spec are marked `APP_DEFINED`.

**2.0.4 Reconciliation policy (soft references).** Wiki re-syncs can change or delete
pageids (the dedup pass deletes losing case-variant pages wholesale — safe precisely
because builds never FK wiki rows). On snapshot load after a `data_version` change, every
build-side wiki reference resolves: (1) by pageid; (2) fallback by `name_canonical`
(pageid then silently updated); (3) unresolved → the selection's `status` becomes
`SAVED_INACTIVE` with `inactive_reason='DATA_MISSING'` — displayed red, contributing 0,
**never deleted** (spec §18 semantics extended to data loss).

**2.0.5 ID and provenance policy.** Wiki entities keyed by MediaWiki pageid
(`spell.id`, `zone.id`, `items.pageid`, `mobs.pageid`). Every wiki-parsed table carries
`source_revision` (revid). Buff-Lines-derived rows default `verified=0`. Overrides win
over parsed rows and are re-applied after every import.

**2.0.6 Importer contract.** Loaders `DELETE` child rows per pageid before re-insert
(kills v1's stale-child bug); spell dedup is case-insensitive on `name_canonical`
(higher revid wins, loser deleted with cascade, both logged `DUPLICATE_TITLE`).

### 2.1 wiki.db DDL (rebuilt wholesale; importer-owned)

```sql
PRAGMA foreign_keys = ON;
PRAGMA user_version = 2;

-- v1 tables kept as-is: items, item_stats, item_classes, item_races, item_categories,
-- mobs, drops, sync_meta, view v_item_class. Extended below via ALTER.

------------------------------------------------------------------------
-- Group 1 — wiki metadata
------------------------------------------------------------------------
-- sync_meta (v1 key-value) reserved keys: 'schema_version'='2', 'last_sync',
-- 'last_sync_spells', 'last_sync_static', 'buff_lines_revid',
-- 'data_stamp' (ISO timestamp + import_run id; builds.db data_version rows point here).

------------------------------------------------------------------------
-- Group 2 — character-system reference tables
------------------------------------------------------------------------
CREATE TABLE class (
  id        INTEGER PRIMARY KEY,               -- seeded 1..16, stable (Group 12)
  abbr      TEXT NOT NULL UNIQUE
            CHECK (abbr IN ('WAR','CLR','PAL','RNG','SHD','DRU','MNK','BRD',
                            'ROG','SHM','NEC','WIZ','MAG','ENC','BST','BER')),
  name      TEXT NOT NULL UNIQUE,
  archetype TEXT NOT NULL CHECK (archetype IN ('MELEE','PRIEST','CASTER','HYBRID'))
);

CREATE TABLE race (
  id            INTEGER PRIMARY KEY,           -- seeded 1..15, stable
  name          TEXT NOT NULL UNIQUE,
  pageid        INTEGER,
  is_unlockable INTEGER NOT NULL DEFAULT 0,    -- +2000 faction achievement  -- verification: WIKI_CONFIRMED
  unlock_notes  TEXT,
  source_revision INTEGER
);

CREATE TABLE race_base_stats (
  race_id INTEGER PRIMARY KEY REFERENCES race(id),
  str INTEGER NOT NULL, sta INTEGER NOT NULL, agi INTEGER NOT NULL, dex INTEGER NOT NULL,
  wis INTEGER NOT NULL, intel INTEGER NOT NULL, cha INTEGER NOT NULL,
  -- verification: WIKI_CONFIRMED (Statistics 15-race table); Kerra row PARTIALLY_VERIFIED
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE class_stat_mod (
  -- per-class ADDITIVE modifiers (Statistics page). How three classes combine is the
  -- builds.db formula key 'class_attr_combine' (default SUM, NEEDS_INGAME_TEST).
  class_id INTEGER PRIMARY KEY REFERENCES class(id),
  str INTEGER NOT NULL DEFAULT 0, sta INTEGER NOT NULL DEFAULT 0, agi INTEGER NOT NULL DEFAULT 0,
  dex INTEGER NOT NULL DEFAULT 0, wis INTEGER NOT NULL DEFAULT 0, intel INTEGER NOT NULL DEFAULT 0,
  cha INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER
);

CREATE TABLE race_class (
  -- race restricts the PRIMARY class only (Newbie Guide)
  race_id  INTEGER NOT NULL REFERENCES race(id),
  class_id INTEGER NOT NULL REFERENCES class(id),
  primary_allowed  INTEGER NOT NULL DEFAULT 0,  -- verification: WIKI_CONFIRMED
  requires_unlock  INTEGER NOT NULL DEFAULT 0,  -- verification: WIKI_CONFIRMED
  source_revision INTEGER,
  PRIMARY KEY (race_id, class_id)
);
-- NOTE: no race/class↔deity legality table exists (wiki does not give one).
-- Deity choice is unconstrained in the app and randomizer until backlog item V17 lands.

CREATE TABLE race_ability (
  id       INTEGER PRIMARY KEY,
  race_id  INTEGER NOT NULL REFERENCES race(id),
  name     TEXT NOT NULL,
  effect_json TEXT,
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER,
  UNIQUE (race_id, name)
);

CREATE TABLE deity (
  id     INTEGER PRIMARY KEY,                   -- seeded 1..17 (16 + Agnostic)
  name   TEXT NOT NULL UNIQUE,
  pageid INTEGER,
  notes  TEXT
);

CREATE TABLE skill (
  id       INTEGER PRIMARY KEY,
  pageid   INTEGER UNIQUE,                      -- 'Skill X' page (~85 exist)
  name     TEXT NOT NULL UNIQUE,
  skill_type TEXT CHECK (skill_type IN ('PASSIVE','WEAPON','CASTING','TRADESKILL','INSTRUMENT','ACTIVE')),
  notes    TEXT,
  source_revision INTEGER
);

CREATE TABLE skill_class (
  skill_id INTEGER NOT NULL REFERENCES skill(id),
  class_id INTEGER NOT NULL REFERENCES class(id),
  acquired_level INTEGER,                       -- verification: WIKI_CONFIRMED (Skills master list)
  source_revision INTEGER,
  PRIMARY KEY (skill_id, class_id)
);

CREATE TABLE skill_cap (
  -- Breakpoint model: rows at documented levels (mostly 50/60); engine interpolates.
  -- User corrections flow through override YAML (wiki.db is disposable), NOT direct edits.
  skill_id INTEGER NOT NULL REFERENCES skill(id),
  class_id INTEGER NOT NULL REFERENCES class(id),
  at_level INTEGER NOT NULL CHECK (at_level BETWEEN 1 AND 60),
  cap_value INTEGER NOT NULL,
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER,
  PRIMARY KEY (skill_id, class_id, at_level)
);

------------------------------------------------------------------------
-- Group 3 — spell core
------------------------------------------------------------------------
CREATE TABLE spell (
  id              INTEGER PRIMARY KEY,          -- MediaWiki pageid
  name            TEXT NOT NULL,
  name_canonical  TEXT NOT NULL,                -- casefold, ws-collapsed, ’→', trailing-.-stripped
  icon            TEXT,
  casting_skill   TEXT,
  mana            INTEGER,
  range           INTEGER,
  casting_time    REAL,
  fizzle_time     REAL,
  recast_time     REAL,
  duration_raw    TEXT,
  target_type_raw TEXT,                         -- verbatim: 'Self','Single','Party','Pet','Group v2','SingleL65','Targeted AE',...
  spell_type_raw  TEXT,                         -- verbatim: 'Beneficial','Detrimental','Statistic Buff','Movement Buff','Pet'
  is_beneficial   INTEGER NOT NULL DEFAULT 0,   -- derived: spell_type_raw <> 'Detrimental'
  resist_type     TEXT CHECK (resist_type IN ('UNRESISTABLE','MAGIC','FIRE','COLD','POISON','DISEASE','VOID')),
  resist_adjust   INTEGER,
  era             TEXT,                         -- verification: PARTIALLY_VERIFIED (~40% tagged; default Classic)
  era_source      TEXT CHECK (era_source IN ('TAG','TABLEERA','CATEGORY','DEFAULT','OVERRIDE')),
  is_npc_only     INTEGER NOT NULL DEFAULT 0,
  is_illusion     INTEGER NOT NULL DEFAULT 0,
  illusion_race_id INTEGER,                     -- game race id from 'Illusion: N' effect
  is_song         INTEGER NOT NULL DEFAULT 0,
  role            TEXT CHECK (role IN ('PET_SUMMON','CONTROL','DAMAGE','PET_BUFF','BUFF','UTILITY')),
     -- APP_DEFINED heuristic (NOT wiki data), computed post-parse from effect opcodes (§3.3):
     -- PET_SUMMON if any SUMMON_PET effect; else CONTROL if any ROOT/MEZ/CHARM/SLOW/FEAR/
     -- STUN/SNARE opcode; else DAMAGE if detrimental with HP/mana-decrease effects; else
     -- PET_BUFF if beneficial + target PET; else BUFF if beneficial + stat-relevant;
     -- else UTILITY. Songs are identified by is_song, orthogonal to role.
  description     TEXT,
  msg_cast_on_you TEXT, msg_cast_on_other TEXT, msg_wears_off TEXT,
  where_to_obtain_raw TEXT,
  other_raw       TEXT,
  template_name   TEXT CHECK (template_name IN ('Spellpage','Spellpagesmart')),
  raw_wikitext    TEXT,
  source_revision INTEGER,
  updated         TEXT
);
CREATE UNIQUE INDEX ux_spell_name_canonical ON spell(name_canonical);
  -- UNIQUE is safe: the higher-revid-wins dedup pass runs before insert (§3.8), and the
  -- linkage ladder (§3.7) depends on this uniqueness.

CREATE TABLE spell_class_level (
  spell_id  INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  class_id  INTEGER NOT NULL REFERENCES class(id),   -- INTEGER via seeded class table (never TEXT abbr)
  required_class_level INTEGER NOT NULL CHECK (required_class_level BETWEEN 1 AND 60),
     -- spec §2 naming law; verification: WIKI_CONFIRMED ('classes' bullets)
  is_autogranted INTEGER NOT NULL DEFAULT 0,         -- verification: WIKI_CONFIRMED
  source_revision INTEGER,
  PRIMARY KEY (spell_id, class_id)
);

CREATE TABLE spell_effect (
  id            INTEGER PRIMARY KEY,
  spell_id      INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  slot_number   INTEGER,                        -- REAL sparse game slot; NULL for the bard pseudo-row
  raw_text      TEXT NOT NULL,
  opcode        TEXT,                           -- open vocabulary: MAX_HP, HP_WHEN_CAST, HP, MANA, AC,
                                                -- ATK, HASTE, MOVE_SPEED, STR..CHA, SUMMON_PET, ILLUSION,
                                                -- ROOT, LEVITATE, *_COUNTER, ADD_PROC, STACKING_RULE,
                                                -- UNKNOWN_STAT, UNPARSED, ...
  stat          TEXT,
  base_amount   REAL,                           -- verification: WIKI_CONFIRMED
  max_amount    REAL,                           -- NULL = flat
  min_caster_level INTEGER,
  max_caster_level INTEGER,
  caster_level_scaling TEXT NOT NULL DEFAULT 'NONE'
    CHECK (caster_level_scaling IN ('NONE','LINEAR_ASSUMED')),
     -- verification: NEEDS_INGAME_TEST (endpoints only; interpolation shape unstated)
  is_percent    INTEGER NOT NULL DEFAULT 0,
  resource_mode TEXT CHECK (resource_mode IN ('MAX','CURRENT','PER_TICK')),
  per_tick_increment REAL,                      -- ramping DoTs (Splurt)
  tier_scaling_json TEXT,
     -- verification: NEEDS_INGAME_TEST — NULL = fall back to formula 'spell_tier_scaling'
  instrument_scaled INTEGER NOT NULL DEFAULT 0, -- verification: PARTIALLY_VERIFIED
  is_cosmetic   INTEGER NOT NULL DEFAULT 0,
  grants_proc_spell_id INTEGER REFERENCES spell(id),
  pet_token     TEXT,                           -- 'skel_pet_9_', 'SumFireR2', 'Animation6', ...
  is_stacking_rule INTEGER NOT NULL DEFAULT 0,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE spell_target_rule (
  spell_id        INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  target_base     TEXT NOT NULL
    CHECK (target_base IN ('SELF','SINGLE','GROUP','PET','CORPSE','AE','UNKNOWN')),
     -- 'AE' covers 'Targeted AE'/'PB AE' (importer normalizes; needed for spec §2's
     -- "affects enemy or environment" display). 'UNKNOWN' = unparsed token + issue row.
  pet_targetable  INTEGER,
     -- can this spell land on a pet when the OWNER casts it?
     -- DEFAULT DERIVATION (importer §3.3): 1 if target_base IN ('SINGLE','GROUP') AND
     -- is_beneficial; 0 if target_base='SELF'; 1 if target_base='PET'; NULL otherwise.
     -- verification: NEEDS_INGAME_TEST (classic-EQ behavior assumed; wiki silent).
     -- Overridable per spell via 'target: {pet_targetable: false}' (§3.6).
  pet_targetable_status TEXT NOT NULL DEFAULT 'NEEDS_INGAME_TEST'
    CHECK (pet_targetable_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                     'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  pet_subtype     TEXT NOT NULL DEFAULT 'ANY'
    CHECK (pet_subtype IN ('ANY','SUMMONED_ONLY','CHARMED_ONLY','OWNER_PET_ONLY')),
     -- verification: NEEDS_INGAME_TEST (override-only)
  target_level_min INTEGER,                     -- verification: NEEDS_INGAME_TEST (override-only)
  target_level_max INTEGER,                     -- verification: WIKI_CONFIRMED ('SingleL65')
  excluded_target_types_json TEXT,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE spell_duration_rule (
  spell_id        INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  duration_class  TEXT NOT NULL CHECK (duration_class IN
    ('INSTANT','PERMANENT','LONG','SHORT','BARD_PULSE','PROC_TRIGGERED','CLICK_TRIGGERED',
     'DISCIPLINE','AURA','MAINTAINED_TOGGLE','UNKNOWN')),
     -- 'UNKNOWN' = unparsed duration text + issue row (importer fallback)
  maintenance_type TEXT CHECK (maintenance_type IN
    ('NORMAL_BUFF','BARD_SONG','BARD_AUTO_PULSE','SHORT_COMBAT_BUFF','PERMANENT_SELF_BUFF')),
  duration_seconds_min INTEGER,
  duration_seconds_max INTEGER,
  duration_min_caster_level INTEGER,
  duration_max_caster_level INTEGER,
  tick_count      INTEGER,
  duration_formula TEXT NOT NULL DEFAULT 'LINEAR_BY_CASTER_LEVEL',
     -- verification: NEEDS_INGAME_TEST (interpolation unstated)
  recast_time     REAL,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE bard_song_rule (                   -- spec §9 verbatim
  spell_id       INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  cast_time      REAL,
  duration_ticks INTEGER,
  instrument_type TEXT CHECK (instrument_type IN ('PERCUSSION','STRINGED','BRASS','WIND','SINGING','ALL','NONE')),
  instrument_scaling_allowed TEXT NOT NULL DEFAULT 'NO'
    CHECK (instrument_scaling_allowed IN ('NO','YES','REQUIRED')),
  is_sustainable INTEGER NOT NULL DEFAULT 1,    -- verification: MANUAL_OVERRIDE (app/user judgment)
  minimum_cycle_time REAL,                      -- verification: NEEDS_INGAME_TEST
  bard_layer     INTEGER,                       -- verification: PARTIALLY_VERIFIED
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE spell_source (
  id          INTEGER PRIMARY KEY,
  spell_id    INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  source_type TEXT NOT NULL CHECK (source_type IN ('VENDOR','DROP','QUEST','RESEARCH','UNKNOWN')),
     -- importer emits 'UNKNOWN' (never 'UNKNOWN_TEXT') for unclassified free text
  zone_name   TEXT,
  npc_name    TEXT,
  area        TEXT,
  loc         TEXT,
  raw_text    TEXT,
  source_revision INTEGER
);

CREATE TABLE spell_item_source (                -- items_with_effect (spell -> item names)
  spell_id  INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  item_name TEXT NOT NULL,
  PRIMARY KEY (spell_id, item_name)
);

CREATE TABLE spell_categories (
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  category TEXT NOT NULL,
  PRIMARY KEY (spell_id, category)
);

------------------------------------------------------------------------
-- Group 4 — buff lines & stacking (spec §5-7)
------------------------------------------------------------------------
CREATE TABLE buff_line (                        -- spec §6 verbatim + provenance
  id       INTEGER PRIMARY KEY,
  name     TEXT NOT NULL UNIQUE,                -- 'STRENGTH', 'AC (Slot 2)', 'PET_HASTE', ...
  category TEXT,                                -- page group OR spec §6 enum incl. PET_* lines
  statistic TEXT,
  effect_slot INTEGER,                          -- verification: PARTIALLY_VERIFIED
  bard_layer  INTEGER,                          -- verification: PARTIALLY_VERIFIED
  selection_policy TEXT NOT NULL DEFAULT 'HIGHEST_EFFECT_VALUE'
    CHECK (selection_policy IN ('HIGHEST_EFFECT_VALUE','HIGHEST_PRIORITY','MANUAL_ONLY')),  -- APP_DEFINED
  notes    TEXT,
  verified INTEGER NOT NULL DEFAULT 0,          -- Buff Lines page self-declared incomplete
  source_revision INTEGER
);
-- PET buff lines (PET_HASTE, PET_AC, PET_STRENGTH, ...) have NO wiki source: the Buff
-- Lines page covers player statistics only. They are created exclusively by the shipped
-- seed file overrides/seeds/pet_buff_lines.yaml (verified=0, NEEDS_INGAME_TEST), which
-- also declares memberships (e.g. the Burnout family under PET_HASTE + PET_STRENGTH).

CREATE TABLE buff_line_member (
  -- Reworked to store what the Buff Lines parser actually produces: surrogate PK,
  -- nullable spell_id for unresolved links, source_kind in the uniqueness key (one line
  -- can list the same effect via SPELL and via Click:/Proc:/Worn:/Consumable: items).
  id           INTEGER PRIMARY KEY,
  buff_line_id INTEGER NOT NULL REFERENCES buff_line(id) ON DELETE CASCADE,
  spell_id     INTEGER REFERENCES spell(id) ON DELETE CASCADE,  -- NULL when unresolved
  member_name_raw TEXT,                         -- link text kept when spell_id IS NULL
  source_kind  TEXT NOT NULL DEFAULT 'SPELL'
    CHECK (source_kind IN ('SPELL','CLICK','PROC','WORN','CONSUMABLE')),
  priority     INTEGER,                         -- bullet order (descending strength)
  effect_value_reference TEXT,                  -- raw '+45 (+126)'
  value_base   REAL,
  value_max_instrument REAL,                    -- bard dual value (max +28/280% instrument)
  source_items TEXT,                            -- JSON array of item names
  is_group INTEGER, is_self_only INTEGER, duration_note TEXT, gm_event INTEGER,
  combination_group_id INTEGER,
  verified INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER,
  UNIQUE (buff_line_id, spell_id, source_kind)
);

CREATE TABLE spell_buff_line (                  -- spec §6 combination buffs, verbatim
  spell_id     INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  buff_line_id INTEGER NOT NULL REFERENCES buff_line(id) ON DELETE CASCADE,
  relationship TEXT NOT NULL CHECK (relationship IN ('PRIMARY','CONSUMES_LINE','STACKS_WITH_LINE','EXCEPTION')),
  verified INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER,
  PRIMARY KEY (spell_id, buff_line_id)
);

CREATE TABLE spell_stacking_rule (              -- spec §7 verbatim + order_dependent
  id           INTEGER PRIMARY KEY,
  spell_id     INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  rule_type    TEXT NOT NULL CHECK (rule_type IN
    ('BLOCK_IF_PRESENT','OVERWRITE_IF_LOWER','OVERWRITE_ALWAYS','BLOCK_IF_HIGHER',
     'MUTUALLY_EXCLUSIVE','STACKS_EXPLICITLY','ILLUSION_EXCLUSIVE','EFFECT_SLOT_CONFLICT')),
  affected_spell_id     INTEGER REFERENCES spell(id),
  affected_buff_line_id INTEGER REFERENCES buff_line(id),
  affected_effect_slot  INTEGER,
  affected_effect_opcode TEXT,
  comparison_operator   TEXT CHECK (comparison_operator IN ('<','<=','=','>=','>','!=')),
  comparison_value      REAL,
  priority     INTEGER NOT NULL DEFAULT 0,
  order_dependent INTEGER NOT NULL DEFAULT 0,   -- Focus of Spirit / Mortal Deftness
  source_type  TEXT NOT NULL CHECK (source_type IN
    ('WIKI_SLOT_ROW','WIKI_PROSE','WIKI_CATEGORY','BUFF_LINES_PAGE','OVERRIDE','INGAME_TEST')),
     -- canonical list; importer emits exactly these (§3.5). Engine trust rank:
     -- INGAME_TEST > WIKI_SLOT_ROW = WIKI_CATEGORY > OVERRIDE > WIKI_PROSE > BUFF_LINES_PAGE
  verified     INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER,
  notes        TEXT
);

------------------------------------------------------------------------
-- Group 5 — item extensions (spec §15 + recon additions)
------------------------------------------------------------------------
ALTER TABLE items ADD COLUMN required_level INTEGER;
   -- 'Required level of N.' (16 epic rows) -- verification: WIKI_CONFIRMED where present; NULL = unstated
ALTER TABLE items ADD COLUMN recommended_level INTEGER;
ALTER TABLE items ADD COLUMN instrument_type TEXT
  CHECK (instrument_type IN ('PERCUSSION','STRINGED','BRASS','WIND','SINGING','ALL'));
ALTER TABLE items ADD COLUMN instrument_resonance INTEGER;
   -- '<Type> Resonance: N' -- verification: value WIKI_CONFIRMED; multiplier curve is a formula
ALTER TABLE items ADD COLUMN slot_normalized TEXT;    -- cleaned tokens ('PRIMARY,SECONDARY')
ALTER TABLE items ADD COLUMN source_revision INTEGER;
-- NOTE: no items.deity_id single column. The 'Deity:' line can name several deities;
-- multi-deity is the join table below (review decision).

CREATE TABLE item_deity (
  item_id  INTEGER NOT NULL REFERENCES items(pageid) ON DELETE CASCADE,
  deity_id INTEGER NOT NULL REFERENCES deity(id),
  PRIMARY KEY (item_id, deity_id)
);  -- 378 items carry 'Deity:' lines -- verification: WIKI_CONFIRMED

CREATE TABLE item_effect (                      -- spec §15 verbatim + provenance
  id             INTEGER PRIMARY KEY,
  item_id        INTEGER NOT NULL REFERENCES items(pageid) ON DELETE CASCADE,
  effect_id      INTEGER REFERENCES spell(id),  -- spec name; spell pageid; NULL if unresolved
  effect_name_raw TEXT NOT NULL,
  activation_type TEXT NOT NULL CHECK (activation_type IN ('WORN','CLICK','PROC','FOCUS','CONSUMABLE')),
  trigger_detail TEXT,
  cast_time      REAL,
  required_level INTEGER,
     -- 'at Level N' suffix (738 items) -- verification: WIKI_CONFIRMED where present
  allowed_classes TEXT,                         -- raw; normalized in item_effect_class;
                                                -- NULL = inherit the host item's classes
  charges        INTEGER,
  match_method   TEXT CHECK (match_method IN ('LINK_TARGET','EXACT','NORMALIZED','BIDIRECTIONAL','UNRESOLVED')),
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE item_effect_class (
  item_effect_id INTEGER NOT NULL REFERENCES item_effect(id) ON DELETE CASCADE,
  class_id       INTEGER NOT NULL REFERENCES class(id),
  PRIMARY KEY (item_effect_id, class_id)
);

CREATE TABLE exaltation (                       -- spec §15 verbatim; NATURAL key (see note)
  source_item_id   INTEGER NOT NULL REFERENCES items(pageid) ON DELETE CASCADE,
  exaltation_type  TEXT NOT NULL CHECK (exaltation_type IN ('ORNAMENTATION','FOCUS','CLICK','WORN','PROC')),
  source_effect_id INTEGER REFERENCES item_effect(id),
  name             TEXT,
  required_level   INTEGER,
     -- per-effect 'at Level N' from the SOURCE item's effect line (the Exaltations page
     -- carries no level data) -- verification: PARTIALLY_VERIFIED
  allowed_classes  TEXT,                        -- raw; normalized in exaltation_class
  allowed_slots    TEXT,                        -- inherits source slot limits (2H proc -> PRIMARY)
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER,
  notes            TEXT,
  PRIMARY KEY (source_item_id, exaltation_type)
);
-- DECISION NOTE: exaltation rows are DERIVED from item_effect on every rebuild; an
-- autoincrement id would not survive the wiki.db atomic swap. The natural key
-- (source_item_id, exaltation_type) is stable, and build_equipment_socket references
-- exaltations by exactly this pair (soft, cross-file).

CREATE TABLE exaltation_class (
  source_item_id  INTEGER NOT NULL,
  exaltation_type TEXT NOT NULL,
  class_id        INTEGER NOT NULL REFERENCES class(id),
  PRIMARY KEY (source_item_id, exaltation_type, class_id),
  FOREIGN KEY (source_item_id, exaltation_type) REFERENCES exaltation(source_item_id, exaltation_type) ON DELETE CASCADE
);

CREATE TABLE exaltation_slot (
  source_item_id  INTEGER NOT NULL,
  exaltation_type TEXT NOT NULL,
  slot            TEXT NOT NULL,
  PRIMARY KEY (source_item_id, exaltation_type, slot),
  FOREIGN KEY (source_item_id, exaltation_type) REFERENCES exaltation(source_item_id, exaltation_type) ON DELETE CASCADE
);

CREATE TABLE exaltation_socket_rule (
  socket_type      TEXT PRIMARY KEY CHECK (socket_type IN ('ORNAMENTATION','FOCUS','CLICK','WORN','PROC')),
  unlock_item_tier INTEGER NOT NULL CHECK (unlock_item_tier BETWEEN 0 AND 10),
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
     -- '+4 fully upgraded' vs 0-10 tier scale inconsistency stored as-is (backlog V13)
  notes            TEXT
);

------------------------------------------------------------------------
-- Group 6 — pets (spec §13-16); reference side (instances live in builds.db)
------------------------------------------------------------------------
CREATE TABLE pet_archetype (
  id       INTEGER PRIMARY KEY,                 -- seeded 1..9, stable (Group 12)
  name     TEXT NOT NULL UNIQUE,
  owner_class_id INTEGER NOT NULL REFERENCES class(id),
  summon_token_pattern TEXT,
  inventory_slots INTEGER,                      -- verification: WIKI_CONFIRMED (Pet Guide)
  dual_wield_unlock_pet_level INTEGER,
     -- verification: WIKI_CONFIRMED for BST (=5); NULL = NEEDS_INGAME_TEST for the other 8 ('?')
  backstab_requires_behind INTEGER NOT NULL DEFAULT 0,
  notes    TEXT,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE pet_intrinsic_class (              -- spec §19; 2 rows per archetype
  pet_archetype_id INTEGER NOT NULL REFERENCES pet_archetype(id) ON DELETE CASCADE,
  class_id         INTEGER NOT NULL REFERENCES class(id),
  PRIMARY KEY (pet_archetype_id, class_id)
);  -- verification: WIKI_CONFIRMED (all 8 pairs quoted on Pet Guide; matches spec §13)

CREATE TABLE pet_archetype_level_stats (
  pet_archetype_id INTEGER NOT NULL REFERENCES pet_archetype(id) ON DELETE CASCADE,
  pet_level INTEGER NOT NULL CHECK (pet_level BETWEEN 1 AND 60),
  hp INTEGER, max_hit INTEGER, ac INTEGER, atk INTEGER,
  harm_touch INTEGER, lifetap INTEGER,
  mitigation TEXT, avoidance TEXT, offense TEXT, accuracy TEXT,
  verification_status TEXT NOT NULL DEFAULT 'NEEDS_INGAME_TEST'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER,
  PRIMARY KEY (pet_archetype_id, pet_level)
);  -- legacy Pet Guide range tables load ONLY as LEGACY_EQ_DATA (and only with --include-legacy)

CREATE TABLE spell_pet_summon (
  spell_id         INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  pet_archetype_id INTEGER REFERENCES pet_archetype(id),
  summon_token     TEXT,
  base_pet_level   INTEGER,                     -- NULL for MAG/ENC/SHM until override/testing
  base_level_source TEXT CHECK (base_level_source IN ('SUMMONEDPETPAGE','OTHER_BLOCK','TOKEN','OVERRIDE')),
  base_pet_level_status TEXT NOT NULL DEFAULT 'NEEDS_INGAME_TEST'
    CHECK (base_pet_level_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                     'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  pet_hp           TEXT,                        -- RAW wiki text: '~75' (approximate values are real data)
  pet_hp_numeric   INTEGER,                     -- derived: digits extracted from pet_hp; NULL if unparseable
  pet_max_hit INTEGER, pet_harm_touch INTEGER, pet_lifetap INTEGER,
  source_revision INTEGER
);

CREATE TABLE pet_stat_block (                   -- {{Summonedpetpage}} rows (BST warders)
  page_pageid INTEGER PRIMARY KEY,
  summon_spell_id INTEGER REFERENCES spell(id),
  pet_archetype_id INTEGER REFERENCES pet_archetype(id),
  level INTEGER, hp INTEGER, hp_regen INTEGER, mana INTEGER, mana_regen INTEGER,
  mitigation INTEGER, avoidance INTEGER, offense INTEGER, accuracy INTEGER,
  str INTEGER, sta INTEGER, agi INTEGER, dex INTEGER, wis INTEGER, intel INTEGER, cha INTEGER,
  max_damage INTEGER, dual_wields TEXT, abilities TEXT,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER
);

CREATE TABLE pet_innate_spell (                 -- spec §16 verbatim
  pet_archetype_id INTEGER NOT NULL REFERENCES pet_archetype(id) ON DELETE CASCADE,
  spell_id         INTEGER NOT NULL REFERENCES spell(id),
  minimum_pet_level INTEGER,                    -- verification: NEEDS_INGAME_TEST
  target_rule      TEXT NOT NULL DEFAULT 'SELF'
    CHECK (target_rule IN ('SELF','OWNER','OWNER_OR_SELF','ENEMY','ANY')),
  automatic        INTEGER NOT NULL DEFAULT 1,
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER,
  PRIMARY KEY (pet_archetype_id, spell_id)
);
-- NOTE: pet_equipment_rule lives in builds.db (user-editable, formula_version FK). §2.2.

------------------------------------------------------------------------
-- Group 7 — zones & farm-list normalization
------------------------------------------------------------------------
CREATE TABLE zone (
  id        INTEGER PRIMARY KEY,                -- MediaWiki pageid
  name      TEXT NOT NULL UNIQUE,
  era       TEXT,
  level_min INTEGER, level_max INTEGER,         -- parsed 'Level of Monsters' -- PARTIALLY_VERIFIED
  level_raw TEXT,
  notes     TEXT,
  source_revision INTEGER,
  updated   TEXT
);

CREATE TABLE zone_unique_item (
  zone_id   INTEGER NOT NULL REFERENCES zone(id) ON DELETE CASCADE,
  item_name TEXT NOT NULL,
  PRIMARY KEY (zone_id, item_name)
);

CREATE TABLE mob_zone (
  mob_pageid INTEGER NOT NULL REFERENCES mobs(pageid) ON DELETE CASCADE,
  zone_id    INTEGER NOT NULL REFERENCES zone(id),
  source     TEXT NOT NULL DEFAULT 'MOB_PAGE' CHECK (source IN ('MOB_PAGE','ZONE_PAGE','OVERRIDE')),
  PRIMARY KEY (mob_pageid, zone_id)
);

ALTER TABLE mobs ADD COLUMN level_min INTEGER;  -- parsed free text -- PARTIALLY_VERIFIED
ALTER TABLE mobs ADD COLUMN level_max INTEGER;
ALTER TABLE mobs ADD COLUMN source_revision INTEGER;
-- Populated by the importer's `normalize-mobs` pass (§3.4b) — the owner the review found missing.

------------------------------------------------------------------------
-- Group 10 — overrides & sync bookkeeping
------------------------------------------------------------------------
CREATE TABLE override (
  id           INTEGER PRIMARY KEY,
  entity_type  TEXT NOT NULL CHECK (entity_type IN
    ('SPELL','ITEM','ITEM_EFFECT','EXALTATION','PET_ARCHETYPE','SPELL_PET_SUMMON','BUFF_LINE',
     'BUFF_LINE_MEMBER','STACKING_RULE','TARGET_RULE','DURATION_RULE','BARD_SONG_RULE','FORMULA',
     'ZONE','MOB','SKILL','SKILL_CAP','RACE','PET_EQUIPMENT_RULE')),
  entity_key   TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  verified     INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER,
  source_file  TEXT,
  applied_at   TEXT,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (entity_type, entity_key, source_file)
);

CREATE TABLE sync_page (
  pageid      INTEGER PRIMARY KEY,
  title       TEXT NOT NULL,
  page_model  TEXT CHECK (page_model IN ('ITEM','MOB','SPELL','SUMMONED_PET','ZONE','RACE','DEITY','SKILL','GUIDE','OTHER')),
  last_revid  INTEGER,
  last_synced TEXT,
  is_deleted  INTEGER NOT NULL DEFAULT 0
);
-- sync_page doubles as the KNOWN_SKILL_TITLES / KNOWN_ZONE_TITLES source for
-- classify_page (§3.2): title sets come from the category walks cached here, NOT from
-- categories_full.json (which has no member titles).

CREATE TABLE import_run (
  id INTEGER PRIMARY KEY,
  started TEXT, finished TEXT, command TEXT, summary_json TEXT
);

CREATE TABLE import_issue (
  id INTEGER PRIMARY KEY,
  run_id INTEGER REFERENCES import_run(id),
  issue_type TEXT NOT NULL,   -- UNPARSED_STACKING, UNRESOLVED_BUFF_MEMBER, DUPLICATE_TITLE,
                              -- UNKNOWN_PET_TOKEN, MISSING_PET_BASE_LEVEL, UNPARSED_DURATION,
                              -- UNKNOWN_TARGET_TYPE, UNPARSED_EFFECT, UNMAPPED_STAT_KEY, ...
  pageid INTEGER, title TEXT, snippet TEXT,
  resolved INTEGER NOT NULL DEFAULT 0,
  created TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE override_application (
  run_id INTEGER, file TEXT, entry_key TEXT,
  target_table TEXT, target_rowkey TEXT, applied TEXT
);

------------------------------------------------------------------------
-- Group 11 — indexes and wiki-local views
------------------------------------------------------------------------
CREATE INDEX idx_scl_class_level   ON spell_class_level(class_id, required_class_level, spell_id);
CREATE INDEX idx_spell_beneficial  ON spell(is_beneficial, is_npc_only);
CREATE INDEX idx_spell_role        ON spell(role);
CREATE INDEX idx_spell_effect_spell ON spell_effect(spell_id, slot_number);
CREATE INDEX idx_spell_source_spell ON spell_source(spell_id);
CREATE INDEX idx_item_classes_class ON item_classes(class);
CREATE INDEX idx_items_slot         ON items(slot_normalized);
CREATE INDEX idx_drops_item         ON drops(item_name);
CREATE INDEX idx_item_effect_item   ON item_effect(item_id);
CREATE INDEX idx_item_effect_spell  ON item_effect(effect_id);
CREATE INDEX idx_blm_spell          ON buff_line_member(spell_id);
CREATE INDEX idx_ssr_spell          ON spell_stacking_rule(spell_id);
CREATE INDEX idx_ssr_affected       ON spell_stacking_rule(affected_spell_id);
CREATE INDEX idx_mob_zone_zone      ON mob_zone(zone_id);
CREATE INDEX idx_override_entity    ON override(entity_type, entity_key);
CREATE INDEX idx_skill_cap_class    ON skill_cap(class_id, skill_id);

-- Item -> dropped-by -> zone (farm backbone; name joins safe: v1 names UNIQUE)
CREATE VIEW v_item_sources AS
SELECT i.pageid AS item_pageid, i.name AS item_name, d.rarity,
       m.pageid AS mob_pageid, m.name AS mob_name,
       m.level_min AS mob_level_min, m.level_max AS mob_level_max, m.level AS mob_level_raw,
       z.id AS zone_id, z.name AS zone_name,
       z.level_min AS zone_level_min, z.level_max AS zone_level_max, z.era AS zone_era
FROM items i
JOIN drops d ON d.item_name = i.name
JOIN mobs  m ON m.name = d.mob_name
LEFT JOIN mob_zone mz ON mz.mob_pageid = m.pageid
LEFT JOIN zone z      ON z.id = mz.zone_id;

------------------------------------------------------------------------
-- Group 12 — seed data (importer writes these on every rebuild; ids are CONSTANTS)
------------------------------------------------------------------------
INSERT INTO class (id, abbr, name, archetype) VALUES
 (1,'WAR','Warrior','MELEE'),(2,'CLR','Cleric','PRIEST'),(3,'PAL','Paladin','HYBRID'),
 (4,'RNG','Ranger','HYBRID'),(5,'SHD','Shadow Knight','HYBRID'),(6,'DRU','Druid','PRIEST'),
 (7,'MNK','Monk','MELEE'),(8,'BRD','Bard','HYBRID'),(9,'ROG','Rogue','MELEE'),
 (10,'SHM','Shaman','PRIEST'),(11,'NEC','Necromancer','CASTER'),(12,'WIZ','Wizard','CASTER'),
 (13,'MAG','Magician','CASTER'),(14,'ENC','Enchanter','CASTER'),(15,'BST','Beastlord','HYBRID'),
 (16,'BER','Berserker','MELEE');

INSERT INTO race (id, name) VALUES
 (1,'Human'),(2,'Barbarian'),(3,'Erudite'),(4,'Wood Elf'),(5,'High Elf'),(6,'Dark Elf'),
 (7,'Half Elf'),(8,'Dwarf'),(9,'Troll'),(10,'Ogre'),(11,'Halfling'),(12,'Gnome'),
 (13,'Iksar'),(14,'Froglok'),(15,'Kerra');

INSERT INTO deity (id, name) VALUES
 (1,'Agnostic'),(2,'Bertoxxulous'),(3,'Brell Serilis'),(4,'Bristlebane'),(5,'Cazic-Thule'),
 (6,'Erollisi Marr'),(7,'Innoruuk'),(8,'Karana'),(9,'Mithaniel Marr'),(10,'Prexus'),
 (11,'Quellious'),(12,'Rallos Zek'),(13,'Rodcet Nife'),(14,'Solusek Ro'),(15,'The Tribunal'),
 (16,'Tunare'),(17,'Veeshan');

INSERT INTO pet_archetype (id, name, owner_class_id, summon_token_pattern, inventory_slots,
                           dual_wield_unlock_pet_level, backstab_requires_behind, verification_status) VALUES
 (1,'Enchanter Animation',      14,'Animation*',  5, NULL,0,'WIKI_CONFIRMED'),
 (2,'Necromancer Skeleton',     11,'skel_pet_*',  6, NULL,0,'WIKI_CONFIRMED'),
 (3,'Shadow Knight Skeleton',    5,'skel_pet_*',  4, NULL,0,'WIKI_CONFIRMED'),
 (4,'Shaman Spirit Wolf',       10,'SpiritWolf*', 5, NULL,0,'WIKI_CONFIRMED'),
 (5,'Magician Air Elemental',   13,'SumAir*',     7, NULL,0,'WIKI_CONFIRMED'),
 (6,'Magician Water Elemental', 13,'SumWater*',   7, NULL,1,'WIKI_CONFIRMED'),
 (7,'Magician Earth Elemental', 13,'SumEarth*',   7, NULL,0,'WIKI_CONFIRMED'),
 (8,'Magician Fire Elemental',  13,'SumFire*',    7, NULL,0,'WIKI_CONFIRMED'),
 (9,'Beastlord Warder',         15,'*',           7, 5,   0,'WIKI_CONFIRMED');
 -- dual_wield NULL = NEEDS_INGAME_TEST (Pet Guide shows '?' post July-7-2026 rework)

INSERT INTO pet_intrinsic_class (pet_archetype_id, class_id) VALUES
 (1,1),(1,3),   -- ENC animation  WAR/PAL
 (2,1),(2,5),   -- NEC skeleton   WAR/SHD
 (3,1),(3,5),   -- SHD skeleton   WAR/SHD
 (4,1),(4,15),  -- SHM spirit     WAR/BST
 (5,1),(5,7),   -- MAG air        WAR/MNK
 (6,1),(6,9),   -- MAG water      WAR/ROG
 (7,1),(7,4),   -- MAG earth      WAR/RNG
 (8,1),(8,12),  -- MAG fire       WAR/WIZ
 (9,1),(9,15);  -- BST warder     WAR/BST

INSERT INTO exaltation_socket_rule (socket_type, unlock_item_tier, verification_status, notes) VALUES
 ('ORNAMENTATION',0,'PARTIALLY_VERIFIED','Exaltations page; +4-vs-+10 tier-scale inconsistency stored as-is'),
 ('FOCUS',        1,'PARTIALLY_VERIFIED',NULL),
 ('CLICK',        2,'PARTIALLY_VERIFIED',NULL),
 ('WORN',         3,'PARTIALLY_VERIFIED',NULL),
 ('PROC',         4,'PARTIALLY_VERIFIED',NULL);

INSERT OR REPLACE INTO sync_meta (key, value) VALUES ('schema_version','2');
```

### 2.2 builds.db DDL (precious; app-migration-owned)

**2.2.0 Soft-reference columns.** Every `*_id` below that names a wiki entity is a plain
INTEGER (no FK possible across files). `item_id`/`spell_id` references carry a
`*_name_canonical` companion filled at selection time; reconciliation per policy 2.0.4.
`class_id`/`race_id`/`deity_id`/`pet_archetype_id` reference the seeded constants.

```sql
PRAGMA foreign_keys = ON;
PRAGMA user_version = 2;

------------------------------------------------------------------------
-- Group 1 — versioning spine (WRITERS specified — the review found none existed)
------------------------------------------------------------------------
CREATE TABLE data_version (
  id            INTEGER PRIMARY KEY,
  label         TEXT NOT NULL UNIQUE,
  wiki_last_sync TEXT,           -- copied from wiki.db sync_meta 'data_stamp'
  notes         TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
-- WRITER: the app's post-swap hook inserts ONE row after every successful wiki.db
-- atomic rename (label = 'dv<N> <ISO date>'), then sets app_meta 'active_data_version'.
-- First-run migration inserts dv1 for the seeded copy.

CREATE TABLE formula_version (
  id            INTEGER PRIMARY KEY,
  label         TEXT NOT NULL UNIQUE,
  notes         TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
-- WRITER: every Settings->Formulas save batch (and every override-apply that touches
-- formula rows) inserts ONE row and stamps the edited formula_table rows; app_meta
-- 'active_formula_version' updated. Migration inserts fv1 with the shipped seeds.

CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT);
-- keys: 'active_data_version', 'active_formula_version', 'builds_schema_version'

------------------------------------------------------------------------
-- Group 2 — the editable formula store (UNIFIED — replaces formula(key,value_json),
-- the importer's formula_table(name,key,value) AND game_rule; curves that the schema
-- draft put in class_level_stats live here as (class, level)-dimensioned rows)
------------------------------------------------------------------------
CREATE TABLE formula_table (
  formula_key   TEXT NOT NULL,
  dim1 TEXT NOT NULL DEFAULT '',   -- e.g. class abbr
  dim2 TEXT NOT NULL DEFAULT '',   -- e.g. level / level band
  dim3 TEXT NOT NULL DEFAULT '',
  value_int     INTEGER,           -- milli-units where fractional (documented round-down)
  value_text    TEXT,              -- for rule choices ('SUM','BEST_OF') and quotes
  description   TEXT,
  is_user_edited INTEGER NOT NULL DEFAULT 0,
  verification_status TEXT NOT NULL
    CHECK (verification_status IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                   'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  formula_version_id INTEGER REFERENCES formula_version(id),
  source        TEXT,              -- wiki page + quote anchoring the value
  PRIMARY KEY (formula_key, dim1, dim2, dim3)
);
```

**Canonical formula key list** (the one registry the schema, importer seeds, engine, and
Settings→Formulas editor all use — the review found three disjoint key sets):

| formula_key | dims | default / seed | verification |
|---|---|---|---|
| `class_attr_combine` | — | `SUM` | NEEDS_INGAME_TEST |
| `multi_class_hp_combine` | — | `BEST_OF` | NEEDS_INGAME_TEST |
| `multi_class_mana_combine` | — | `BEST_OF` | NEEDS_INGAME_TEST |
| `multi_class_skill_combine` | — | `BEST_OF` | NEEDS_INGAME_TEST |
| `base_hp` | (class, level) | classic-EQ curve seed | NEEDS_INGAME_TEST |
| `hp_per_sta` | (class, level) | L50/L60 wiki anchors, linear between | PARTIALLY_VERIFIED at anchors |
| `base_mana` | (class, level) | classic-EQ seed | NEEDS_INGAME_TEST |
| `mana_per_stat` | (class, level) | ~11/pt @L60 anchor | PARTIALLY_VERIFIED |
| `item_tier_stat_bonus` | — | +10%/tier, floor, min +1 | WIKI_CONFIRMED |
| `item_tier_weapon` | — | dmg +10%/tier min 1; delay never reduced; weight floor 0.1 | WIKI_CONFIRMED |
| `item_tier_xp_cost` | — | 2^n ladder; fodder 2^tier | WIKI_CONFIRMED |
| `spell_tier_scaling` | — | +10%/tier, round down, min +1 (assumption mirroring items) | NEEDS_INGAME_TEST |
| `pet_level_rule` | — | `MIN(base + tier, char_level - 1)` | WIKI_CONFIRMED |
| `pet_upgrade_per_level` | — | +6% HP, +1 dmg, +5 skill pts | WIKI_CONFIRMED |
| `instrument_multiplier` | (resonance 0..28) | 29 rows, linear ×1.0→×2.8 | NEEDS_INGAME_TEST (top anchor WIKI_CONFIRMED) |
| `ac_softcap` | (level) | `level*6+25` | LEGACY_EQ_DATA |
| `resist_percent` | — | 6 pts = 1%, 5/95 bounds | LEGACY_EQ_DATA |
| `softcap_model` | — | report-only above 200 | NEEDS_INGAME_TEST |
| `stat_caps` | — | hard 255; soft 200 WIS/INT/CHA | WIKI_CONFIRMED |
| `buff_slot_limit` | — | 15 incl. bard songs | WIKI_CONFIRMED |
| `pet_buff_slot_limit` | — | 15 (assumed same) | NEEDS_INGAME_TEST |
| `buff_target_level_floor` | (spell_max_level) | ≤52→40, ≤60→45 | WIKI_CONFIRMED |
| `effect_caster_level_interpolation` | — | `LINEAR` | NEEDS_INGAME_TEST |
| `bard_conservative_song_count` | — | 3 | MANUAL_OVERRIDE (app default) |
| `bard_twist_buffer_milli` | — | 1000 (1.0 s) | MANUAL_OVERRIDE |
| `short_duration_threshold_s` | — | 300 | MANUAL_OVERRIDE |
| `optimizer_weights` | (profile, stat_class) | §4.4 table | MANUAL_OVERRIDE |
| `pet_focus_rule` | — | off by default ("needs testing") | LEGACY_EQ_DATA |

The importer's `game_rule` quotes (buff limit, pet rank rule, proc-level rule, …) become
`formula_table` rows with the quote in `source`. **The app's assumption banner reads
these rows and states: "Attribute modifiers: sum of the three classes; HP / mana / skill
caps: best-of-three" — never "best-of-three per stat" wholesale** (review fix).

```sql
------------------------------------------------------------------------
-- Group 3 — pet_equipment_rule (versioned, user/override-editable -> builds.db)
------------------------------------------------------------------------
CREATE TABLE pet_equipment_rule (               -- spec §14: versioned rows; HIGHEST id wins
  id INTEGER PRIMARY KEY,
  include_pet_classes   INTEGER NOT NULL DEFAULT 1,  -- WIKI_CONFIRMED (Pet Guide quote)
  include_owner_classes INTEGER NOT NULL DEFAULT 1,  -- WIKI_CONFIRMED
  respect_deity_restrictions INTEGER,                -- NULL = unknown -> warn-only (NEEDS_INGAME_TEST)
  effect_class_source TEXT NOT NULL DEFAULT 'PET_INTRINSIC_ONLY'
    CHECK (effect_class_source IN ('PET_INTRINSIC_ONLY','FULL_POOL')),  -- PARTIALLY_VERIFIED
  formula_version INTEGER REFERENCES formula_version(id),
  verified INTEGER NOT NULL DEFAULT 0,
  notes    TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- SEEDED by the builds.db migration (row 1 = documented values above), NOT by the
-- importer (wiki.db is disposable). If a future sync detects the Pet Guide equipment
-- paragraph changed, an import_issue('WIKI_CHANGED_PET_RULE') prompts the user; changes
-- land as new rows here (via UI or override apply). Engine reads MAX(id).

------------------------------------------------------------------------
-- Group 4 — builds (spec §1, §13, §18)
------------------------------------------------------------------------
CREATE TABLE build (                            -- spec §1 verbatim + timestamps
  id              INTEGER PRIMARY KEY,
  name            TEXT NOT NULL,
  character_level INTEGER NOT NULL DEFAULT 1 CHECK (character_level BETWEEN 1 AND 50),
  race_id         INTEGER,                      -- soft ref (seeded constant)
  deity_id        INTEGER,                      -- soft ref
  buff_mode       TEXT NOT NULL DEFAULT 'OFF' CHECK (buff_mode IN ('OFF','AUTO','CUSTOM')),
  spell_tier_default INTEGER NOT NULL DEFAULT 0 CHECK (spell_tier_default BETWEEN 0 AND 10),
  active_player_buff_profile_id INTEGER,        -- app-validated (circular)
  active_pet_buff_profile_id    INTEGER,
  data_version    INTEGER REFERENCES data_version(id),      -- spec column name
  formula_version INTEGER REFERENCES formula_version(id),
  created_at      TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at      TEXT
);

CREATE TABLE build_class (
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  slot     TEXT NOT NULL CHECK (slot IN ('PRIMARY','SECONDARY','TERTIARY')),
  class_id INTEGER NOT NULL,                    -- soft ref (seeded constant)
  PRIMARY KEY (build_id, slot),
  UNIQUE (build_id, class_id)
);

CREATE TABLE pet_instance (                     -- spec §13 verbatim + reconciliation cols
  id                INTEGER PRIMARY KEY,
  build_id          INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  summon_spell_id   INTEGER NOT NULL,           -- soft ref -> wiki spell pageid
  summon_spell_name_canonical TEXT,             -- reconciliation fallback (2.0.4)
  summon_spell_tier INTEGER NOT NULL DEFAULT 0 CHECK (summon_spell_tier BETWEEN 0 AND 10),
  base_pet_level    INTEGER,                    -- copied at selection; NULL if unknown
  calculated_pet_level INTEGER,                 -- NULL when base is unknown (engine models this)
  primary_class_id   INTEGER,                   -- denormalized intrinsic pair (spec §13)
  secondary_class_id INTEGER,
  pet_archetype_id   INTEGER,
  status            TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','SAVED_INACTIVE')),
  inactive_reason   TEXT                        -- 'summon requires level 16' / 'DATA_MISSING'
);

CREATE TABLE build_equipment (
  id             INTEGER PRIMARY KEY,
  build_id       INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  equipped_by    TEXT NOT NULL DEFAULT 'PLAYER' CHECK (equipped_by IN ('PLAYER','PET')),
  pet_instance_id INTEGER REFERENCES pet_instance(id) ON DELETE CASCADE,
  slot           TEXT NOT NULL CHECK (slot IN
    ('HEAD','FACE','EAR1','EAR2','NECK','SHOULDERS','ARMS','BACK','WRIST1','WRIST2','HANDS',
     'FINGER1','FINGER2','CHEST','LEGS','FEET','WAIST','PRIMARY','SECONDARY','RANGE','AMMO',
     'BANDOLIER_1','BANDOLIER_2','BANDOLIER_3','BANDOLIER_4')
    OR slot GLOB 'PET_INV_*'),
     -- BANDOLIER_1..4 (player only): carried-not-worn instruments. They contribute ONLY
     -- instrument resonance to song scaling (§4.4) — no stats, no worn effects. This is
     -- how a Bard's twisting drum is modeled without occupying PRIMARY (review fix);
     -- buff_profile_spell.instrument_modifier_override remains the manual escape hatch.
  item_id        INTEGER NOT NULL,              -- soft ref -> items.pageid
  item_name_canonical TEXT,                     -- reconciliation fallback
  item_upgrade_tier INTEGER NOT NULL DEFAULT 0 CHECK (item_upgrade_tier BETWEEN 0 AND 10),
  is_acquired    INTEGER NOT NULL DEFAULT 0,    -- 0 = wishlist -> farm list
  status         TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','SAVED_INACTIVE')),
  inactive_reason TEXT,                         -- 'INVALID_LEVEL','INVALID_CLASS','INVALID_SLOT',
                                                -- 'DUAL_WIELD_UNAVAILABLE','DATA_MISSING',...
  CHECK ((equipped_by = 'PLAYER' AND pet_instance_id IS NULL)
      OR (equipped_by = 'PET'    AND pet_instance_id IS NOT NULL))
);
CREATE UNIQUE INDEX ux_be_player ON build_equipment(build_id, slot) WHERE equipped_by = 'PLAYER';
CREATE UNIQUE INDEX ux_be_pet    ON build_equipment(pet_instance_id, slot) WHERE equipped_by = 'PET';
CREATE INDEX idx_be_build ON build_equipment(build_id);

CREATE TABLE build_equipment_socket (
  id                 INTEGER PRIMARY KEY,
  build_equipment_id INTEGER NOT NULL REFERENCES build_equipment(id) ON DELETE CASCADE,
  socket_type        TEXT NOT NULL CHECK (socket_type IN ('ORNAMENTATION','FOCUS','CLICK','WORN','PROC')),
  exaltation_source_item_id INTEGER,            -- soft NATURAL key -> wiki exaltation
  exaltation_type           TEXT,               --   (source_item_id, exaltation_type)
  exaltation_name           TEXT,               -- display fallback if source item vanishes
  status             TEXT NOT NULL DEFAULT 'ACTIVE'
    CHECK (status IN ('ACTIVE','SAVED_INACTIVE','SOCKET_LOCKED')),
  inactive_reason    TEXT,
  UNIQUE (build_equipment_id, socket_type)
);

CREATE TABLE build_spell_tier (
  -- Build-scoped per-spell tier (review fix: the Spells-page tier stepper had no home
  -- for non-buff spells). PRECEDENCE (engine input ST):
  --   buff_profile_spell.spell_upgrade_tier (profile override)
  --   > build_spell_tier.spell_upgrade_tier (this table)
  --   > build.spell_tier_default
  build_id  INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  spell_id  INTEGER NOT NULL,                   -- soft ref
  spell_name_canonical TEXT,
  spell_upgrade_tier INTEGER NOT NULL CHECK (spell_upgrade_tier BETWEEN 0 AND 10),
  PRIMARY KEY (build_id, spell_id)
);

CREATE TABLE build_wishlist (
  -- Farm-list wants beyond equipped-but-unacquired: alternative candidates for one slot,
  -- spell scrolls ('Spell: X' Itempage rows — store the scroll's pageid), pet spares.
  id        INTEGER PRIMARY KEY,
  build_id  INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  item_id   INTEGER NOT NULL,                   -- soft ref -> items.pageid
  item_name_canonical TEXT,
  target    TEXT NOT NULL DEFAULT 'PLAYER' CHECK (target IN ('PLAYER','PET')),
  note      TEXT,
  added_at  TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (build_id, item_id, target)
);

------------------------------------------------------------------------
-- Group 5 — buff profiles & persisted calculations (spec §4, §12, §19)
------------------------------------------------------------------------
CREATE TABLE buff_profile (                     -- spec §4 verbatim + bard mode (§9)
  id       INTEGER PRIMARY KEY,
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  name     TEXT NOT NULL,
  mode     TEXT NOT NULL DEFAULT 'AUTO' CHECK (mode IN ('OFF','AUTO','CUSTOM')),
  target_type TEXT NOT NULL CHECK (target_type IN ('PLAYER','PET')),
  include_short_duration   INTEGER NOT NULL DEFAULT 0,
  include_bard_songs       INTEGER NOT NULL DEFAULT 1,
  include_item_clicks      INTEGER NOT NULL DEFAULT 1,
  include_consumables      INTEGER NOT NULL DEFAULT 0,
  include_pet_innate_buffs INTEGER NOT NULL DEFAULT 1,
  maximum_buff_slots INTEGER NOT NULL DEFAULT 15,   -- WIKI_CONFIRMED (stated twice)
  optimization_profile TEXT NOT NULL DEFAULT 'BALANCED' CHECK (optimization_profile IN
    ('BALANCED','MAX_HP','MAX_AC','MAX_MELEE','MAX_CASTER','MAX_RESISTS','MAX_REGEN','MAX_PET_HP','MAX_PET_DAMAGE')),
  bard_maintenance_mode TEXT NOT NULL DEFAULT 'IDEAL_SUSTAINED'
    CHECK (bard_maintenance_mode IN ('NONE','CONSERVATIVE','IDEAL_SUSTAINED','CUSTOM')),
  UNIQUE (build_id, name)
);

CREATE TABLE buff_profile_spell (               -- spec §4 verbatim + reconciliation col
  buff_profile_id INTEGER NOT NULL REFERENCES buff_profile(id) ON DELETE CASCADE,
  spell_id        INTEGER NOT NULL,             -- soft ref
  spell_name_canonical TEXT,
  spell_upgrade_tier INTEGER NOT NULL DEFAULT 0 CHECK (spell_upgrade_tier BETWEEN 0 AND 10),
  selection_mode  TEXT NOT NULL CHECK (selection_mode IN
    ('AUTO_SELECTED','MANUALLY_ENABLED','MANUALLY_DISABLED','REPLACED_BY_STRONGER',
     'REJECTED_STACKING','INVALID_TARGET','INVALID_LEVEL')),
  is_enabled      INTEGER NOT NULL DEFAULT 1,
  caster_level_override INTEGER,
  instrument_modifier_override REAL,
  PRIMARY KEY (buff_profile_id, spell_id)
);

CREATE TABLE build_calculation (
  -- ONE ROW PER **PERSISTED** CALCULATION — i.e. profile save or debug-trace export ONLY
  -- (review fix: NOT one row per pipeline run; the engine memoizes in RAM and avoids
  -- OneDrive-era write-churn habits). calculation_id is the in-memory input fingerprint,
  -- which becomes this row's natural identity at persist time.
  id             INTEGER PRIMARY KEY,
  fingerprint    TEXT NOT NULL,                 -- engine calculation_id (u64 hex)
  build_id       INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  character_level INTEGER NOT NULL,
  player_buff_profile_id INTEGER REFERENCES buff_profile(id),
  pet_buff_profile_id    INTEGER REFERENCES buff_profile(id),
  pet_instance_id        INTEGER REFERENCES pet_instance(id),
  data_version    INTEGER REFERENCES data_version(id),
  formula_version INTEGER REFERENCES formula_version(id),
  result_json     TEXT,                         -- spec §12 build_calculation_result
  formula_confidence TEXT,
  created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE resolved_buff (                    -- spec §19 verbatim; CHECKs widened to
  id             INTEGER PRIMARY KEY,           -- cover the engine's full status space
  calculation_id INTEGER NOT NULL REFERENCES build_calculation(id) ON DELETE CASCADE,
  target_type    TEXT NOT NULL CHECK (target_type IN ('PLAYER','PET')),
  spell_id       INTEGER NOT NULL,              -- soft ref
  spell_tier     INTEGER NOT NULL DEFAULT 0 CHECK (spell_tier BETWEEN 0 AND 10),
  caster_level   INTEGER,
  target_level   INTEGER,
  status         TEXT NOT NULL CHECK (status IN
    ('ACTIVE','REJECTED_WEAKER','REJECTED_STACKING','REJECTED_TARGET','REJECTED_LEVEL',
     'REJECTED_SLOT_LIMIT','REJECTED_PROFILE','REJECTED_NOT_MAINTAINABLE',
     'REJECTED_NOT_BENEFICIAL','REJECTED_OTHER','SAVED_INACTIVE','MANUALLY_DISABLED')),
  calculated_effects_json TEXT,
  rejection_reason TEXT,
  conflicting_spell_id INTEGER,
  stacking_rule_id     INTEGER,                 -- soft ref (wiki rule id; snapshot value)
  source        TEXT CHECK (source IN
    ('OWNER_SPELL','PET_INNATE_SPELL','ITEM_CLICK','ITEM_WORN','EXALTATION','CONSUMABLE'))
     -- full engine BuffSource set; the spec §16 three-source display collapses
     -- ITEM_CLICK/ITEM_WORN/EXALTATION/CONSUMABLE -> "ITEM_OR_EXALTATION_EFFECT"
);
CREATE INDEX idx_rb_calc ON resolved_buff(calculation_id, target_type);
```

**Engine → resolved_buff.status mapping** (binding; review fix — every engine outcome
now has a CHECK-legal value):

| Engine `BuffStatus` / `RejectReason` | status |
|---|---|
| Active | ACTIVE |
| OverwrittenByStronger | REJECTED_WEAKER |
| BlockedByRule, MutuallyExclusive, LineConflict, ConsumedLineConflict, IllusionExclusive, SlotConflict | REJECTED_STACKING |
| InvalidTarget, TargetLevel | REJECTED_TARGET |
| BelowLevel | REJECTED_LEVEL |
| BuffCapReached | REJECTED_SLOT_LIMIT |
| ExcludedByProfile | REJECTED_PROFILE |
| NotMaintainable | REJECTED_NOT_MAINTAINABLE |
| NotBeneficial, NotStatRelevant | REJECTED_NOT_BENEFICIAL |
| SavedInactive | SAVED_INACTIVE |
| ManuallyDisabled | MANUALLY_DISABLED |
| (any future variant) | REJECTED_OTHER + rejection_reason text |

```sql
CREATE TABLE resolved_buff_conflict (
  id             INTEGER PRIMARY KEY,
  calculation_id INTEGER NOT NULL REFERENCES build_calculation(id) ON DELETE CASCADE,
  target_type    TEXT NOT NULL CHECK (target_type IN ('PLAYER','PET')),
  winner_spell_id INTEGER,
  loser_spell_id  INTEGER,
  conflict_type  TEXT NOT NULL CHECK (conflict_type IN
    ('BLOCK_IF_PRESENT','OVERWRITE_IF_LOWER','OVERWRITE_ALWAYS','BLOCK_IF_HIGHER','MUTUALLY_EXCLUSIVE',
     'ILLUSION_EXCLUSIVE','EFFECT_SLOT_CONFLICT','BUFF_LINE_EXCLUSIVE','COMBINATION_CONSUMES_LINE',
     'SLOT_LIMIT_EXCEEDED')),
  buff_line_id   INTEGER,
  effect_slot    INTEGER,
  stacking_rule_id INTEGER,
  description    TEXT
);
```

**Engine `ConflictKind` → conflict_type mapping** (binding): Blocks →
BLOCK_IF_PRESENT/BLOCK_IF_HIGHER (per the firing rule's rule_type) · Overwrites →
OVERWRITE_ALWAYS/OVERWRITE_IF_LOWER (per rule) · MutuallyExclusive → MUTUALLY_EXCLUSIVE ·
LineExclusive → BUFF_LINE_EXCLUSIVE · ConsumedLine → COMBINATION_CONSUMES_LINE ·
SlotConflict → EFFECT_SLOT_CONFLICT · Illusion → ILLUSION_EXCLUSIVE · Cap →
SLOT_LIMIT_EXCEEDED.

```sql
CREATE TABLE pet_item_validation (              -- spec §19; item-level badges (spec §17)
  id              INTEGER PRIMARY KEY,
  calculation_id  INTEGER NOT NULL REFERENCES build_calculation(id) ON DELETE CASCADE,
  pet_instance_id INTEGER NOT NULL REFERENCES pet_instance(id),
  build_equipment_id INTEGER REFERENCES build_equipment(id),
  item_id         INTEGER NOT NULL,             -- soft ref
  can_store       INTEGER NOT NULL DEFAULT 0,
  can_equip       INTEGER NOT NULL DEFAULT 0,
  can_use_stats   INTEGER,                      -- NULL = unknown (pet level unknown); rule §4.6.2
  badge           TEXT NOT NULL CHECK (badge IN
    ('FULLY_ACTIVE','EQUIPPABLE_PROC_INACTIVE','EQUIPPABLE_EXALTATION_INACTIVE',
     'INVALID_CLASS','INVALID_PET_LEVEL','INVALID_SLOT','DUAL_WIELD_UNAVAILABLE')),
  reason          TEXT,
  rule_verification TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED'
    CHECK (rule_verification IN ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST',
                                 'MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME'))
);

CREATE TABLE pet_effect_validation (            -- spec §19; per-effect stages (spec §14/15)
  id              INTEGER PRIMARY KEY,
  calculation_id  INTEGER NOT NULL REFERENCES build_calculation(id) ON DELETE CASCADE,
  pet_instance_id INTEGER NOT NULL REFERENCES pet_instance(id),
  build_equipment_id INTEGER REFERENCES build_equipment(id),
  item_effect_id  INTEGER,                      -- soft refs (snapshot values)
  exaltation_source_item_id INTEGER,
  exaltation_type TEXT,
  stage           TEXT NOT NULL CHECK (stage IN
    ('CAN_ACTIVATE_WORN_EFFECT','CAN_ACTIVATE_PROC','CAN_ACTIVATE_EXALTATION')),
  class_match     INTEGER,                      -- PARTIALLY_VERIFIED (transferred-class edge cases)
  level_match     INTEGER,                      -- WIKI_CONFIRMED ('pets ALWAYS respect proc levels')
  slot_match      INTEGER,
  tier_match      INTEGER,
  passed          INTEGER NOT NULL DEFAULT 0,
  required_level  INTEGER,
  pet_level       INTEGER,
  reason          TEXT                          -- 'Proc inactive: pet requires level 37'
);
```

### 2.3 Cross-database TEMP views (created on the app connection after ATTACH wiki)

```sql
-- Spell availability per build (spec §2 rule; merged-record grouping by spell_id)
CREATE TEMP VIEW v_spell_available AS
SELECT b.id AS build_id, s.id AS spell_id, s.name AS spell_name, c.abbr AS class_abbr,
       scl.required_class_level, scl.is_autogranted,
       s.is_beneficial, s.is_song, s.role, s.era,
       (scl.required_class_level <= b.character_level) AS is_available
FROM build b
JOIN build_class bc            ON bc.build_id = b.id
JOIN wiki.spell_class_level scl ON scl.class_id = bc.class_id
JOIN wiki.class c              ON c.id = bc.class_id
JOIN wiki.spell s              ON s.id = scl.spell_id
WHERE s.is_npc_only = 0;

-- Pet item class pool (spec §14: intrinsic UNION owner, gated by current versioned rule)
CREATE TEMP VIEW v_pet_item_class_pool AS
SELECT pi.id AS pet_instance_id, pi.build_id, pic.class_id, 'PET_INTRINSIC' AS pool_source
FROM pet_instance pi
JOIN wiki.pet_intrinsic_class pic ON pic.pet_archetype_id = pi.pet_archetype_id
WHERE (SELECT include_pet_classes FROM pet_equipment_rule ORDER BY id DESC LIMIT 1) = 1
UNION
SELECT pi.id, pi.build_id, bc.class_id, 'OWNER_CLASS'
FROM pet_instance pi
JOIN build_class bc ON bc.build_id = pi.build_id
WHERE (SELECT include_owner_classes FROM pet_equipment_rule ORDER BY id DESC LIMIT 1) = 1;

-- Farm list = equipped-but-unacquired UNION wishlist (review fix), joined to sources
CREATE TEMP VIEW v_build_farm_list AS
SELECT be.build_id, 'EQUIPMENT' AS want_kind, be.id AS want_id, be.equipped_by AS target,
       be.slot, be.item_upgrade_tier, vs.*
FROM build_equipment be
JOIN wiki.v_item_sources vs ON vs.item_pageid = be.item_id
WHERE be.is_acquired = 0
UNION ALL
SELECT w.build_id, 'WISHLIST', w.id, w.target, NULL, NULL, vs.*
FROM build_wishlist w
JOIN wiki.v_item_sources vs ON vs.item_pageid = w.item_id;
```

### 2.4 Migration & run-order notes

1. `docs/schema.sql` is generated from this section and is the ONLY DDL artifact. The
   **importer** executes the wiki.db section when building `wiki.db.building` (so M0 does
   not depend on M1); the **app migration runner** executes the builds.db section at
   first launch (M1). Neither invents tables.
2. wiki.db is rebuilt from scratch every full sync (fresh file → parse → seeds →
   overrides → sanity gate → atomic rename). Incremental syncs update in place with
   per-pageid child pre-deletes, then re-apply overrides.
3. builds.db migrations are additive and versioned via `app_meta.builds_schema_version`.
4. The two unenforced circular references (`build.active_*_buff_profile_id`) are
   app-validated, as are all cross-file soft references (policy 2.0.4).

---

## 3. Importer v2 — `scripts/eql_wiki_sync.py`

Invariants carried from v1: fetch → raw cache → parse → upsert separation (parse/load
stdlib-only, re-runnable offline from `raw/*.json`; only override paths import PyYAML);
polite single-threaded pacing (0.5 s, 5-retry backoff, same UA); flat category walking;
pageid-keyed upserts; era stored, never filtered.

**The importer defines NO tables.** It executes the wiki.db section of `docs/schema.sql`
(§2.1) and writes to those tables verbatim. All stamped `verification_status` values are
the canonical six (§2.0.2): template-parsed fields → `WIKI_CONFIRMED`; Buff Lines seed →
`NEEDS_INGAME_TEST` (+ `verified=0`); prose-parsed rules → `verified=0`; legacy Pet Guide
tables → `LEGACY_EQ_DATA` (only with `--include-legacy`); override-applied →
`MANUAL_OVERRIDE`, or `VERIFIED_INGAME` when the entry carries `tested.method: in_game`.

### 3.1 Subcommands

```
sync [--gear] [--mobs] [--category X] [--incremental]        (v1, dispatch now total)
sync-spells   [--category NAME] [--skip-npc] [--incremental] [--no-overrides] [--strict]
sync-static   [--group GROUP[,GROUP]] [--page TITLE] [--no-overrides] [--strict]
reparse-items [--from db|raw] [--no-overrides]
normalize-mobs                                                (NEW — see 3.4b)
apply-overrides [--dir overrides/] [--dry-run]
check-overrides [--dir overrides/] [--strict]
report [--run RUN_ID]
load --from-raw <file>   /   export   /   map-categories     (v1, export adds spells/pets/static/issues JSON)
```

Canonical run order:

```
map-categories                       # optional refresh
sync --gear --mobs                   # v1 unchanged
reparse-items                        # new item columns, offline
normalize-mobs                       # mob level ranges + mob_zone, offline
sync-spells                          # spells + pet templates + linkage pass
sync-static --group all              # buff lines, pet guide, races, deities, classes,
                                     # skills, mechanics, zones
check-overrides && export
# daily: sync --incremental          # now covers every page type
```

`apply_overrides()` runs automatically at the end of `sync-spells`, `sync-static`,
`reparse-items`, `normalize-mobs`, and any `sync --incremental` that touched overridable
tables (wiki → overrides → stamp; a re-sync can never silently clobber curation — the
wiki's new value surfaces as a `WIKI_CHANGED_UNDER_OVERRIDE` warning). The spell↔item
linkage pass (3.7) runs after `sync-spells`/`reparse-items` (idempotent, DB→DB).

`sync-spells` walks `["Spells", "NPC Only Spells", "Summoned Pet", "Beastlord Pet"]`
(~2,605 pages ≈ 55 requests). `sync-static` groups: `buff-lines` (page 50578) ·
`pet-guide` (page 50581) · `races` (Category Character Races + Statistics) · `deities` ·
`classes` (Character Classes, Newbie Guide) · `skills` (Category Skills) · `mechanics`
(Game Mechanics, Item Upgrade System, Exaltations) · `zones` (Category Zones) · `all`.

### 3.2 `classify_page` dispatch (total; replaces v1's two-way substring check)

```python
def classify_page(pid, title, text):
    if title in STATIC_PAGES:            return ("static", STATIC_PAGES[title])
    if "{{Namedmobpage"    in text:      return ("mob", parse_mob)
    if "{{Itempage"        in text:      return ("item", parse_item)
    if "{{Summonedpetpage" in text:      return ("petpage", parse_summonedpetpage)
    if "{{Spellpagesmart"  in text:      return ("spell", parse_spell_smart)   # BEFORE Spellpage
    if "{{Spellpage"       in text:      return ("spell", parse_spell)
    if title.startswith("Skill ") and title in KNOWN_SKILL_TITLES: return ("skill", parse_skill_page)
    if title in KNOWN_ZONE_TITLES:       return ("zone", parse_zone_page)
    return ("skipped", None)             # counted + sampled in the run report, never silent
```

**Review fix:** `KNOWN_SKILL_TITLES` / `KNOWN_ZONE_TITLES` are populated from the
category-member walks themselves (titles cached into `sync_page` during
`sync-static --group skills/zones`) — NOT from `categories_full.json`, which contains
only category names/sizes. Expect **~85** actual `Skill X` pages in the 170-page Skills
category; the other ~85 route to the counted `skipped` bucket.

### 3.3 Spell template parsing — field mapping onto §2.1

One parser for both templates via the existing `parse_template_fields` brace-walker.
Class link targets map to the seeded `class.id` (Warrior→1 … Berserker→16); **class_id is
always the INTEGER FK, never a TEXT abbr**.

| Wiki param | §2.1 column(s) | Transform / fallback |
|---|---|---|
| pageid, revid | `spell.id`, `spell.source_revision` | direct (`rvprop=ids\|content`) |
| `spellname` | `spell.name`, `spell.name_canonical` | normalizer of 3.7; fallback page title |
| `classes` bullets | `spell_class_level(spell_id, class_id, required_class_level, is_autogranted)` | regex `\*\s*\[\[([^\]\|]+)...\]\]\s*-\s*Level\s*(\d+)(\(Autogranted\))?`; NPC prose → `is_npc_only=1`; unparsed → issue `UNPARSED_CLASSES` |
| `slots` rows | `spell_effect` rows (`slot_number`, `raw_text`, parsed cols) | grammar 3.5a; `Enhanced by instrument?` pseudo-row → `bard_song_rule.instrument_scaling_allowed`, no effect row; `Stacking:` rows → `is_stacking_rule=1` + rule row |
| `skill` | `spell.casting_skill` | strip links |
| `mana`/`range`/`casting_time`/`fizzle_time`/`recast_time` | same-name columns | numeric |
| `duration` | `spell.duration_raw` + `spell_duration_rule` | grammar incl. `X @Lmin to Y @Lmax`; tick=6 s; classes: INSTANT / BARD_PULSE (songs) / SHORT (≤90 s) / LONG / PERMANENT; **unparsed → `duration_class='UNKNOWN'`** (CHECK-legal per §2.1) + issue `UNPARSED_DURATION` |
| `target_type` | `spell.target_type_raw` + `spell_target_rule` | Self→SELF; Single→SINGLE; `SingleL(\d+)`→SINGLE+`target_level_max`; Party/`Group v2`→GROUP; Pet→PET; Corpse→CORPSE; **`Targeted AE`/`PB AE`→`AE`** (CHECK-legal); unknown→`UNKNOWN`+issue. Then derive `pet_targetable` per the §2.1 default rule |
| `spell_type` | `spell.spell_type_raw`, `is_beneficial` | `is_beneficial = 0 iff raw=='Detrimental'` |
| `resist` | `resist_type`, `resist_adjust` | `^(\w[\w ]*?)\s*(?:\(([+-]?\d+)\))?$` |
| `msg_*` | same-name columns | raw |
| `items_with_effect` | `spell_item_source` | `{{:Item}}` transclusion names |
| `other` bullets | `spell_pet_summon` (`Pet Level/Classes/Type/Hit Points/...`; **`pet_hp` stays TEXT — '~75' is real data; digits extracted to `pet_hp_numeric`**), reagent/focus notes | absent on most pages |
| `where_to_obtain` | `spell_source` | `SpellWhereRow` → VENDOR; free bullets → DROP; keywords → QUEST/RESEARCH; **other → `UNKNOWN`** (never UNKNOWN_TEXT) + raw |
| leading `{{X Era}}` | `spell.era`, `era_source='TAG'` | fallbacks TABLEERA → CATEGORY → DEFAULT ('Classic'; count surfaced in report) |
| `[[Category:...]]` | `spell_categories`; Illusions → `is_illusion` + ILLUSION rule; `X line` (6 exist) → `spell_buff_line` seed | |

**Post-parse derivations** (stored, never recomputed by the app): duration/target rules,
`is_beneficial`, `is_song` (BRD class AND instrument/singing skill or pseudo-slot; mana 0
corroborates), opcode/stat per 3.5a, cosmetic flag, and **`spell.role`** — the
APP_DEFINED heuristic of §2.1 that backs the Spells-page Damage / Control / Utility
filters (review fix; it is a heuristic, not wiki data, and is labeled as such in the UI).

### 3.4 `reparse-items` (offline) — including the canonicalization pass

New statsblock rules: `Required level of (\d+)` / `REQ LEVEL (\d+)` →
`items.required_level`; `Recommended level of (\d+)` → `recommended_level`;
`Deity:\s*(.+)` → **`item_deity` rows** (split on commas/'and'; names resolved against
the seeded deity table); `(Brass|Wind|Percussion|Stringed|All) ... Resonance:? (\d+)` →
`instrument_type` + `instrument_resonance`; full effect-line grammar (3.7) →
`item_effect` rows with `at Level N` → `required_level`.

**Canonicalization pass (review fix — v1 dirty data the engine would otherwise consume):**

| Dirty input | Rule |
|---|---|
| `item_stats` key typos | explicit map: `SV POISION`→`SV POISON`, `SV VOID` kept, etc.; unmapped keys → issue `UNMAPPED_STAT_KEY` + coverage line in the run report |
| `item_races` tokens `ALL<BR>`, `VAH<BR>` | strip `<BR>`/HTML before matching |
| `item_races` `None`/`NONE`/`All` | normalize: ALL = no restriction; NONE = unusable (report) |
| `item_races` `except` constructions | emit issue `RACE_EXCEPT_CLAUSE` (curation), do not guess |
| `item_classes` `ALL` / `NON` | ALL = all 16 classes; NON = no class (unwearable) — documented semantics |
| slot case chaos (`FACE`, `Primary Secondary`) | `items.slot_normalized` canonical tokens |
| `Template:NPC` ingested as mob | namespace filter (ns != 0) deletes/blocks template-namespace rows |

### 3.4b `normalize-mobs` (NEW — the owner the review found missing)

Runs inside `sync --mobs` and standalone (offline). (1) Parses `mobs.level` free text
(`52 - 53`, `less than 31`, `70 (55 Pre-revamp)`, `7-20 / 11-13`) into
`level_min`/`level_max`; unparseable → issue `UNPARSED_MOB_LEVEL`, never guessed.
(2) Splits `mobs.zone` comma lists into `mob_zone` rows resolved against `zone` by name
(`source='MOB_PAGE'`); zone pages' Unique Items lists add `zone_unique_item`. Coverage
target: ≥95% of mobs get a numeric range (M6 gate).

### 3.5 Stacking-text parser → `spell_stacking_rule`

Emitted `source_type` values are exactly the §2.1 CHECK list:

**(a) Structured slot rows — `source_type='WIKI_SLOT_ROW'`, `verified=1`.** Grammar
(both verbs accepted; only Block observed in sample):

```
Stacking: (Block new spell | Overwrite existing spell)
          if slot <N> is effect '<EffectName>' [ and (<|<=|>|>=) <value> ]
```

→ `BLOCK_IF_PRESENT` (with slot/opcode/op/value) or `OVERWRITE_IF_LOWER` /
`OVERWRITE_ALWAYS`. The opcode string normalizes through the same effect-stat table as
3.5a so `affected_effect_opcode` joins `spell_effect.opcode`. A Stacking: row also keeps
its `spell_effect` row (`is_stacking_rule=1`) so slot occupancy stays complete.

**(b) Prose scanner over descriptions — `source_type='WIKI_PROSE'`, `verified=0`.**
P1/P2 `Does NOT stack with [[X]]` → MUTUALLY_EXCLUSIVE per resolved link; P3/P5
`replacing [[X]]...` / `all-in-one` → OVERWRITE_ALWAYS + combination candidate; P4
`stacks with [[X]]` (not negated) → STACKS_EXPLICITLY; P6 stacking keyword with no
resolvable link → **no rule row**, issue `UNPARSED_STACKING` (override queue). Same for
unresolvable links, compound sentences, order-dependent semantics (Focus of Spirit →
`spell_buff_line` EXCEPTION row + issue).

**(c) Category rules — `source_type='WIKI_CATEGORY'`, `verified=1`.** Every
`Category:Illusions` member gets one ILLUSION_EXCLUSIVE rule against the ILLUSION line.

Buff-line exclusivity itself is NOT materialized per-pair; the engine's step 9 reads
`buff_line_member`. Buff Lines footnote exceptions become `source_type='BUFF_LINES_PAGE'`
rows. Override-created rules are `source_type='OVERRIDE'`; in-game-confirmed rules
`'INGAME_TEST'`.

### 3.6 Buff Lines page → lines/members + the pet-line seeds

Page 50578 parsed per the tokenizer pseudocode of the importer draft (headings h2/h3/h4 →
groups/statistics/lines; `Slot`/`Layer` names → `effect_slot`/`bard_layer`; bullets →
members). Everything from this page: `verified=0`, `NEEDS_INGAME_TEST`, page revid.
Member bullets: `+45 (+126)` dual values → `value_base`/`value_max_instrument`;
`Click:/Proc:/Worn:/Consumable:` (tolerating `Click::`) → `source_kind`+`source_items`;
class-level annotations cross-check `spell_class_level` (mismatch → issue, never
overwrites); unresolved links → **row still written with `spell_id=NULL` +
`member_name_raw`** (the §2.1 rework makes this insertable) + issue
`UNRESOLVED_BUFF_MEMBER`. Combination detection: spell in ≥2 lines → PRIMARY on its
largest-effect line, CONSUMES_LINE elsewhere, shared `combination_group_id`, issue
`COMBINATION_CANDIDATE`; summary tables skipped (derived data) except the literal
`Combination` column; Focus of Spirit → EXCEPTION + issue. Buff Lines pre-delete: all
rows with the previous page revid, lines upserted by name (stable ids for overrides).

**Pet buff lines (review fix — no wiki source exists):** the repo ships
`overrides/seeds/pet_buff_lines.yaml` creating `buff_line` rows `PET_HASTE`, `PET_AC`,
`PET_STRENGTH` (+ `PET_REGEN`, `PET_DAMAGE_SHIELD` stubs) with memberships for the known
owner-cast pet-buff families — at minimum the Burnout family (Burnout → PET_HASTE +
PET_STRENGTH priority 2; Burnout II priority 1) so spec §21's walkthrough and AUTO's
"strongest of each line" work for pets. All `verified=0`, `NEEDS_INGAME_TEST`. Listed in
the shipped-seed inventory (3.6b). Without these seeds pet buffs degrade to unlined
candidates resolved purely by slot conflicts — golden test 2 asserts both paths (§4.9).

**3.6b Shipped seed overrides (`overrides/seeds/`, committed):** spell-tier scaling stub;
instrument multiplier 29-row assumption; multiclass combination defaults (attr SUM;
HP/mana/skills BEST_OF); MAG/ENC/SHM pet base-level stubs (named spells, NULL values —
the run report lists them under `MISSING_PET_BASE_LEVEL` until filled); pet dual-wield
unknowns; duration interpolation assumption; **pet_buff_lines.yaml** (above);
pet-target-rule note (`pet_effect_class_match`); exaltation socket tiers note.

### 3.7 Spell↔item linkage (`item_effect`)

Inputs: `items.worn_effect/click_effect/focus_effect` + reparsed raw effect lines +
reverse `spell_item_source`. Effect-line grammar:
`Effect: [[Spell]] (Worn|Combat|Must Equip|Any Slot[,...]) [, Casting Time: N|Instant]
[at Level N | req. level N]`. Activation: source column refined by hints (Combat→PROC,
Must Equip/Any Slot+cast→CLICK, Worn→WORN, expendable→CONSUMABLE).

Shared name normalizer (also used by 3.5b and 3.6): link **target** over display; strip
parentheticals/level suffixes/`Spell: ` prefix; casefold; collapse whitespace; `’`→`'`;
strip trailing `.`.

Match ladder (recorded in `match_method`): LINK_TARGET → EXACT → NORMALIZED →
BIDIRECTIONAL upgrade (verified=1) → UNRESOLVED (row kept, `effect_id=NULL`, issue). No
fuzzy matching. Normalized collisions cannot survive import because
`ux_spell_name_canonical` is UNIQUE (§2.1) **and** the dedup pass (3.8) runs first — the
UNIQUE index now enforces what the draft merely asserted.

### 3.8 Idempotency, dedup, incremental

- Parents: `INSERT ... ON CONFLICT(pageid) DO UPDATE`. Children: pre-`DELETE` per
  pageid before insert (spell_class_level, spell_effect, spell_source,
  spell_item_source, spell_categories, WIKI_* stacking rules; override/BuffLines rows
  owned by their own passes). `reparse-items` retrofits the same pattern to item children.
- Duplicate names: uniqueness on `name_canonical`; on collision the **higher revid wins**,
  loser page deleted with child cascade, both logged `DUPLICATE_TITLE`. Safe for builds
  because builds never FK wiki rows and reconcile by name_canonical (§2.0.4).
- `sync --incremental`: total dispatch means changed spell/pet/static pages now land.
  `sync_meta` keys `last_sync`, `last_sync_spells`, `last_sync_static`,
  `buff_lines_revid`. A changed Buff Lines page triggers full 3.6 re-import +
  `check-overrides` staleness pass. Deletions/renames: mitigated by dedup + issues; the
  full category re-walk (~55 requests) is the reconciliation tool.

### 3.9 Override files (spec §20 format, extended)

Directory `overrides/**.yaml`; each file a list; sorted-path order, later files win,
`priority` breaks ties. Discriminators: `spell | pet | item | formula | game_rule |
buff_line`. Entries carry `verified`, `source_revision`, `tested: {by, date, method}`
(method `in_game | wiki | assumption`). Patch sections per entry: `fields`, `target`
(incl. `pet_targetable`, `pet_subtype`, `target_level_min`), `duration`, `effects` (by
slot), `stacking` (`compatible_with`/`incompatible_with`/`exclusive_lines`/`blocks`/
`overwrites`), `buff_lines`, pet `base_levels`/`intrinsic_classes`/`innate_spells`, item
`item_effects`, `formula` rows.

Stamping on apply: `verification_status='VERIFIED_INGAME'` when `tested.method==in_game`,
else `'MANUAL_OVERRIDE'`; every application logged to `override_application` (full chain:
wiki revid → override file → row). Idempotent: absolute values; override-sourced rows
deleted and re-created each apply.

`check-overrides` validates: YAML → schema/enums (against the §2 canonical lists) →
reference resolution (nearest-name suggestions) → conflicts → staleness (page revid >
entry revision → WARN) → redundancy (INFO) → coverage nags (open import_issues no
override addresses). Exit 0/1(strict)/2.

### 3.10 Run report

Every command ends with `write_run_report()` → console + `import_run.summary_json`:
pages fetched/parsed/skipped (sampled titles); per-domain row counts; era-source
histogram (DEFAULTED% surfaced); pet base-level source counts + `MISSING` queue; issue
counts by type; **verification histogram using the canonical enum** (feeds the app's
data-health screen and spec §12 `formula_confidence`); overrides applied/stale/unknown.
`--strict` exits 1 on ERROR-class issues.

---

## 4. Calculation & stacking engine

### 4.1 Placement decision: pure Rust crate `eql-engine` (canonical; the TS-engine variant is dead)

```
app/
  src-tauri/
    crates/
      eql-engine/   # PURE: types, pipeline, resolver, optimizer, randomizer. No I/O, no SQLite.
      eql-data/     # rusqlite loaders: wiki.db + builds.db -> immutable Arc<StaticData> snapshot
    src/main.rs     # Tauri commands (4.1b)
  src/              # webview (Svelte/TS): view-models only, ZERO game rules
```

Rationale (why Rust wins over the app doc's pure-TS engine): the stacking resolver and
optimizer demand property tests, exhaustive enum matching (a new `rule_type` variant
fails compilation until every arm handles it), no `undefined` through a 15-stage
pipeline, and integer-only stat math. `eql-data` loads everything once per
`data_version` into an `Arc<StaticData>` (~few MB), so slider ticks never touch SQLite
and unit tests construct snapshots as literals. Test stack: **proptest + insta +
criterion** (Vitest remains only for webview view-model tests). Types are shared
generation-only: `serde` + `ts-rs` derive `.d.ts` mirrors that cannot drift. The crate
is I/O-free and compiles to WASM unchanged (escape hatch if IPC latency ever matters).

Determinism contract: no HashMap iteration order reaches outputs (BTreeMap or sorted at
the boundary); no floats in stat math (i32, milli-units, documented round-down); no wall
clock; RNG only the seeded PCG32 (§6); result embeds data/formula/engine versions; equal
inputs ⇒ byte-identical serialized result (test-asserted).

**4.1b Tauri command surface**

```
get_snapshot(data_version)               -> snapshot metadata (engine holds the Arc)
recalculate(build_inputs)                -> emits event `calculation-updated` (result object)
compare_items(build, slot, candidate_item_ids[]) -> Vec<ItemCompareDelta>
   // REVIEW FIX: powers the Equipment picker's per-row unbuffed/buffed deltas and the
   // compare drawer. Reuses S10..S14 with one substituted EQ entry (the dirty matrix's
   // 4-stage path); batched over candidates for SQL-paged picker lists.
choose_for_me(constraints, seed?)        -> generated build (+ seed share-code)
save_build / save_buff_profile / save_pet_instance
save_formula_rows(edits[])               -> inserts formula_version row, restamps, recalcs
persist_calc_trace(calculation_id)       -> writes build_calculation + resolved_buff(+conflict)
                                            + pet_*_validation rows (§2.2 persist-only policy)
query_items(filters, sort, page) / query_mob_drops(item_ids) / get_item(id)
```

The webview holds inputs in `$state`, debounces `recalculate`, and every page renders
from the single immutable result carried by `calculation-updated` (spec §12).

### 4.2 The immutable result object

```rust
pub struct CalcVersions { data_version: String, formula_version: String, engine_version: String }
pub type SpellId = u32;  pub type ItemId = u32;  pub type ClassId = u8;

pub enum FormulaConfidence {           // 1:1 mirror of §2.0.2, canonical order best->worst
    VerifiedIngame, WikiConfirmed, ManualOverride,
    PartiallyVerified, NeedsIngameTest, LegacyEqData,
}
pub enum TargetKind { Player, Pet }

pub struct BuildCalculationResult {    // spec §12, verbatim fields + extensions
    pub calculation_id: u64,           // input fingerprint; becomes the persisted row id
    pub versions: CalcVersions,
    pub unbuffed_character: StatBlock,
    pub buffed_character:   StatBlock,
    pub unbuffed_pet:       Option<PetStatBlock>,
    pub buffed_pet:         Option<PetStatBlock>,
    pub active_player_buffs:   Vec<ResolvedBuff>,
    pub rejected_player_buffs: Vec<ResolvedBuff>,
    pub active_pet_buffs:      Vec<ResolvedBuff>,
    pub rejected_pet_buffs:    Vec<ResolvedBuff>,
    pub conflicts:             Vec<BuffConflict>,
    pub spell_availability:  Vec<AvailableSpell>,
    pub bard_maintenance:    Option<BardMaintenanceReport>,
    pub equipment_warnings:     Vec<EquipmentWarning>,
    pub pet_equipment_warnings: Vec<EquipmentWarning>,
    pub pet_item_validation:    Vec<PetItemValidation>,   // §17 badges + CAN_* stages
    pub formula_confidence: FormulaConfidenceReport,
}

pub struct StatLine {                  // spec §10 breakdown — REVIEW FIX: exaltations is
    pub base: i32,                     // its own bucket, matching the app's rendered line
    pub equipment: i32,
    pub item_tiers: i32,
    pub worn_effects: i32,             // item-native worn effects only
    pub exaltations: i32,              // worn/focus-type Exaltation socket contributions
    pub buffs: i32,
    pub raw_total: i32,                // = base+equipment+item_tiers+worn_effects+exaltations+buffs
    pub cap: Option<i32>,
    pub effective: i32,
    pub over_cap: i32,
    pub confidence: FormulaConfidence,
}

pub struct StatBlock { level: u8, stats: Vec<(Stat, StatLine)>,
                       skills: Vec<SkillLine>, derived_notes: Vec<FormulaNote> }

pub enum PetLevel {                    // REVIEW FIX: MAG/ENC/SHM base levels are wiki-
    Known(u8),                         // ABSENT; the engine models "unknown" explicitly
    Unknown { reason: &'static str },  // instead of a fake u8
}
pub struct PetStatBlock {
    pub archetype_id: u32,
    pub summon_spell_id: SpellId,
    pub base_pet_level: LeveledValue,          // value + confidence
    pub calculated_pet_level: PetLevel,        // Known(MIN(base+tier, L-1)) or Unknown
    pub intrinsic_classes: [ClassId; 2],
    pub stats: StatBlock,                      // empty-with-banner when level Unknown
    pub dual_wield: DualWieldStatus,           // Unlocked | LockedUntil(u8) | Unknown
    pub inventory_slots: u8,
}
// Unknown propagation: S5 yields Unknown -> pet stats render "level unknown — enter base
// level (Settings->Formulas / override)"; Stage-B level checks return Unknown-WARN (never
-- silent pass/fail); proc/dual-wield/target-floor checks badge NEEDS_INGAME_TEST.

pub enum BuffStatus { Active, Rejected, SavedInactive, ManuallyDisabled }
pub enum RejectReason {                // persisted per the §2.2 status mapping table
    BelowLevel { required: u8 }, NotBeneficial, NotStatRelevant,
    InvalidTarget { target_type: String },
    TargetLevel { floor: u8, target_level: u8 },
    BlockedByRule { rule_id: u32, by: SpellId },
    OverwrittenByStronger { by: SpellId },
    MutuallyExclusive { rule_id: u32, with: SpellId },
    LineConflict { line_id: u32, with: SpellId },
    ConsumedLineConflict { line_id: u32, combo: SpellId },
    SlotConflict { slot: u8, with: SpellId },
    IllusionExclusive { with: SpellId },
    BuffCapReached { cap: u8 },
    NotMaintainable { mode: BardMaintenanceMode },
    ExcludedByProfile { flag: &'static str },
}

pub struct ResolvedBuff {
    pub target_type: TargetKind, pub spell_id: SpellId,
    pub source: BuffSource,   // OwnerSpell{class} | PetInnate | ItemClick{item} |
                              // ItemWorn{item} | Exaltation{item} | Consumable
    pub spell_tier: u8, pub caster_level: u8, pub target_level: u8,
    pub status: BuffStatus, pub selection_mode: SelectionMode,
    pub calculated_effects: Vec<CalculatedEffect>,
    pub buff_lines: Vec<(u32, LineRelationship)>,
    pub occupied_slots: Vec<u8>,
    pub reject_reason: Option<RejectReason>,
    pub conflicting_spell_id: Option<SpellId>,
    pub stacking_rule_id: Option<u32>,
    pub score: i64,
    pub confidence: FormulaConfidence,
}

pub struct CalculatedEffect { slot: u8, opcode: EffectOpcode, stat: Option<Stat>,
    base_value: i32, tier_value: i32, final_value: i32,
    per_tick: bool, affects_max_resource: bool, cosmetic_only: bool,
    instrument_multiplier_milli: Option<u32> }

pub struct BuffConflict { target_type: TargetKind, winner: SpellId, loser: SpellId,
    kind: ConflictKind,   // maps to resolved_buff_conflict.conflict_type per §2.2 table
    stacking_rule_id: Option<u32> }

pub enum TargetNorm { Self_, Single, Group, Pet, Corpse, Ae, Unknown }
   // REVIEW FIX: Ae added (Targeted/PB AE) so the Spells page can render
   // "affects enemy or environment"; Unknown carries the raw string.

pub struct AvailableSpell {
    pub spell_id: SpellId,
    pub per_class: Vec<(ClassId, u8, bool)>,   // (class, required_class_level, autogranted)
    pub usable_now: bool,
    pub target_norm: TargetNorm,
    pub role: SpellRole,        // PET_SUMMON|CONTROL|DAMAGE|PET_BUFF|BUFF|UTILITY (spell.role)
    pub is_song: bool, pub npc_only: bool,
}

pub enum WarningKind { SavedInactive, IllegalSelection, ProcInactive, ExaltationInactive,
                       DualWieldUnavailable, StatsInactivePetLevel, UnverifiedRule }
   // StatsInactivePetLevel = CAN_USE_STATS failed (§4.6.2): worn but stats contribute 0
pub struct EquipmentWarning { target: TargetKind, item_id: ItemId, slot: EquipSlot,
    kind: WarningKind, reason: String, becomes_valid_at: Option<u8> }

pub struct FormulaConfidenceReport {
    pub overall: FormulaConfidence,
    pub per_formula: Vec<(String, FormulaConfidence, String)>,
    pub unverified_contributions: Vec<(Stat, i32)>,
}
```

**Persistence:** `resolved_buff` / conflict / validation rows always exist in the
in-memory result; they are written to builds.db **only** via `persist_calc_trace`
(profile save or debug export) — §2.2's build_calculation policy.

### 4.3 Pipeline stages, dependency graph, memoization

Build inputs: **L** level · **C** classes ·**R** race · **D** deity · **EQ** player
equipment (incl. BANDOLIER slots) · **PEQ** pet equipment · **PS** pet summon
(spell, tier) · **ST** spell tiers (precedence: profile override > `build_spell_tier` >
`spell_tier_default` — §2.2) · **BPp/BPt** buff profiles · **BM** bard mode · **FT**
formula tables · **DV** static snapshot.

| Stage | Computes | Inputs |
|---|---|---|
| S1 | validated level | L |
| S2 | AvailableSpell[] | C, L, DV |
| S3 | effective tier per spell | S2, ST |
| S4 | pet summon identity + validity flag | PS, S2 |
| S5 | calculated_pet_level (PetLevel::Known/Unknown), archetype, intrinsics | S4, PS.tier, L, FT |
| S6 | player buff candidates | S3, BPp, EQ, BM |
| S7 | pet buff candidates | S3, S5, PEQ, BPt, DV |
| S8 | player stacking resolution + optimizer | S6, L, BPp, BM, EQ(instruments), FT, DV |
| S9 | pet stacking resolution | S7, S5, L, BPt, FT, DV |
| S10 | unbuffed character stats | R, C, L, D, EQ, FT |
| S11 | buffed character stats | S8, S10, FT |
| S12 | unbuffed pet stats + item/proc validation (incl. CAN_USE_STATS) | S5, PEQ, C, FT, DV |
| S13 | buffed pet stats | S9, S12 |
| S14 | caps + derived stats + over-cap | S11, S13, FT |
| S15 | assemble result, emit `calculation-updated` | all |

Dirty matrix (what one edit recomputes) — unchanged from the engine draft, with the
guarantees: **level slider** dirties S1-S3, S5-S15 but NOT S4's identity (spec §18: the
selected summon is preserved; only its validity flag flips); **one gear swap** is 4
stages (S10, S11, S14, S15) unless the item carries buff effects or instrument resonance;
**pet-only edits** never touch the player side.

**Farm list (review fix — explicitly outside the engine):** farm-list *contents* derive
from `build_equipment (is_acquired=0)` + `build_wishlist` via the `v_build_farm_list`
TEMP view and are **level-invariant**; the Farm List page re-queries the view on every
`calculation-updated` event, and only its *annotations* (mob-level-vs-character red
coloring, red inactive wants) recompute from `calcResult`. Spec §1's "farm-list contents"
slider entry is satisfied by this re-query + re-annotation, not by an engine stage.

Memoization: per-stage input fingerprints (hash of relevant field subset + upstream
output fingerprints); a stage re-executes iff its fingerprint changed; identical outputs
stop the dirty wave. Structural sharing via `Arc`. Fine-grained memos: sorted
`avail_index[class]` prefix-merge for S2; bounded LRU
`eval_cache[(spell, caster_level, tier, instr_milli)]`; optimizer memo keyed by
candidate-set fingerprint + profile. Invalidation only by fingerprint; FT/DV versions are
in every fingerprint.

### 4.4 The stacking resolver (spec §7's 12 steps)

Contract:

```
resolve_buffs(target: TargetKind, target_level: u8|Unknown, build_level: u8,
              candidates: Vec<Candidate>,   // ORDERED by optimizer or manual intent
              profile: &BuffProfile, rules: &RulesIndex, formulas: &FormulaTables)
  -> Resolution { active, rejected, conflicts }
```

Deterministic, non-searching; never evicts an incumbent except through an explicit
OVERWRITE_* rule. CUSTOM mode: manual enables head the order; conflicting manual picks
produce visible conflicts (spec §3). Candidate carries spell ref, source, effective tier,
and (songs) best equipped instrument resonance — **computed as the max resonance across
equipped items of the song's instrument type, including the BANDOLIER_1..4 slots**
(§2.2 — the "unequipped twisting drum" model; there is no other bandolier concept), with
`buff_profile_spell.instrument_modifier_override` winning when set.

Resolver state: `lines_occupied` (BTreeMap line → holder+relationship, incl. lines
consumed by combination buffs) · `slots_occupied` keyed by **(layer, slot)** where layer
= Song for BARD_SONG types, Spell otherwise (Buff Lines "Layer 2" model) ·
`illusion_holder` (ILLUSION is a capacity-1 line) · `active` insertion-ordered (cast
order = acceptance order, needed for order-dependent EXCEPTIONs).

```
for cand in candidates:                          # order fixed by caller
  # profile pre-filters (profile semantics, not game rules)
  if cand.manually_disabled:                       reject(ManuallyDisabled); continue
  if short_duration(spell) and !profile.include_short_duration and !cand.manually_enabled:
                                                   reject(ExcludedByProfile); continue
  ... include_item_clicks / include_consumables / include_bard_songs / include_pet_innate ...

  # STEP 1 availability: OwnerSpell required_class_level vs build_level;
  #        PetInnate min_pet_level vs target_level; Item*/Exaltation "at Level N" vs holder level
  # STEP 2 beneficial + stat-relevant (spell_type != Detrimental; cosmetic-only filtered)
  # STEP 3 target compatibility (spell_target_rule):
  #        target==PET  -> requires pet_targetable=1 (PET target always; SINGLE/GROUP per
  #                        the seeded default, confidence NEEDS_INGAME_TEST carried on the
  #                        row); SELF never lands on the pet
  #        target==PLAYER -> reject pet-only spells (Burnout)
  #        target_level_max ('SingleL65'); buff_target_level_floor(L51+ table);
  #        pet_subtype restrictions (override-only rules)
  #        target_level Unknown (pet) -> level-dependent checks return WARN-Unknown, buff
  #        excluded from totals, badge NEEDS_INGAME_TEST (never a silent pass)
  # STEP 4 effects at caster level (linear interpolation between (Lx)..(Ly), clamped;
  #        shape itself NEEDS_INGAME_TEST via formula)
  # STEP 5 spell_upgrade_tier via formula spell_tier_scaling (NEEDS_INGAME_TEST default
  #        +10%/tier round-down min +1)
  # STEP 6 bard instruments: for songs with scaling allowed, multiply all effect
  #        components EXCEPT Haste and ManaRegen (WIKI_CONFIRMED exclusion) by
  #        instrument_multiplier(resonance) (29-row editable lookup);
  #        'Required' scaling + no instrument -> candidate drops out
  # STEP 7 explicit block/overwrite rules, BOTH directions:
  #        7a incumbents' rules vs candidate (e.g. Aegolism's slot-3 <1100 block)
  #        7b candidate's rules vs incumbents (OVERWRITE_IF_LOWER value compare;
  #           OVERWRITE_ALWAYS -> evictions)
  # STEP 8 pairwise exceptions: MUTUALLY_EXCLUSIVE reject; STACKS_EXPLICITLY whitelist;
  #        EXCEPTION{order_dependent} checked against acceptance order
  #        (Focus of Spirit stacks with Mortal Deftness only if MD accepted first)
  # STEP 9 buff-line exclusivity incl. combination buffs:
  #        holder is combo + candidate PRIMARY on a consumed line -> ConsumedLineConflict;
  #        ILLUSION = implicit capacity-1 line for Category:Illusions members
  # STEP 10 effect-slot conflicts: same (layer, slot) occupied by non-whitelisted,
  #        non-evicted holder -> SlotConflict
  # STEP 11 buff cap: active - evictions >= profile.maximum_buff_slots (15, incl. songs,
  #        WIKI_CONFIRMED twice) -> BuffCapReached
  # STEP 12 commit: apply evictions (mark OverwrittenByStronger + conflict rows),
  #        occupy lines/slots, push active, emit ResolvedBuff{Active}
```

Every rejection emits a full ResolvedBuff (reason, conflicting spell, rule id) — the
spec §19 debugging record feeding the §11 tabs. Rule trust rank for confidence badging
uses the canonical source_type order (§2.1): INGAME_TEST > WIKI_SLOT_ROW = WIKI_CATEGORY
> OVERRIDE > WIKI_PROSE > BUFF_LINES_PAGE; rejections fired by `verified=0` rules carry
degraded confidence into the UI.

### 4.5 The optimizer (spec §8, AUTO mode)

Scoring: `score(cand) = Σ marginal_gain(effect) × weight[profile][stat_class]`, with
`marginal_gain = min(final_value, cap_headroom vs the UNBUFFED block)` (cap-aware,
order-independent). The 9-profile weight table ships as `formula_table` rows
(`optimizer_weights`, editable); BALANCED encodes the spec's priority order (HP/Mana →
AC → attributes → haste → regen → resists → DS). STA routes through projected HP only
when the STA→HP formula confidence permits; otherwise flat weight (no false precision).

Search: (1) filter through resolver steps 1-3 + profile flags; (2) score + total-order
tie-breakers; (3) partition by buff line (`line_best[line]`; unlined candidates —
including pet buffs when the PET_* seed lines are absent — pass straight through by
score); (4) conflict hypergraph over combination buffs (CONSUMES_LINE rows); (5) exact
subset search per (small) component; (6) global set; (7) bard gate (§4.5b) caps songs at
maintainable n; (8) cap gate top-15 by score+tie-breakers; (9) validate through the
resolver (thresholds, cross-statistic slot collisions, order-dependent exceptions);
(10) bounded repair loop — substitute next-best same-line candidate per rejection;
terminates (each iteration permanently retires one candidate), converges in 0-2
iterations in practice.

Tie-breakers (spec §8, lexicographic total order): 1 fewer buff slots → 2 longer
duration → 3 source rank OwnerSpell < PetInnate < ItemWorn < ItemClick < Consumable →
4 higher verification confidence → 5 higher tier investment → 6 spell pageid ascending.
Same build ⇒ same result, always.

Pet profiles (`MAX_PET_HP`/`MAX_PET_DAMAGE`) are ordinary weight columns used by the
independent `target=PET` run: separate candidates, separate cap (`pet_buff_slot_limit`,
default 15, NEEDS_INGAME_TEST), possibly different profile (spec §4).

**4.5b Bard maintenance (spec §9).** Classification per import (§3.3):
BARD_SONG/BARD_AUTO_PULSE/SHORT_COMBAT_BUFF/PERMANENT_SELF_BUFF/NORMAL_BUFF. Maintainable
count: IDEAL_SUSTAINED `n = floor(min_duration / max(cast_time, min_cycle))` (defaults
18s/3s → 6); CONSERVATIVE `n = min(configured count [default 3], floor(min_duration /
(avg_cast + twist_buffer 1.0s)))`; NONE → 0; CUSTOM → user picks = manual enables.
Songs beyond n reject `NotMaintainable`. Maintained songs occupy the Song slot-layer and
count against the 15 cap. `BardMaintenanceReport` carries mode, n, per-song cycle math,
and the mandated banner string "Assumption: Ideal sustained Bard song rotation".
Instrument model per 4.4 step 6; the resonance→multiplier 29-row table is
NEEDS_INGAME_TEST with the +28⇒280% anchor WIKI_CONFIRMED; Buff Lines dual values
(`+35 (+98)`) are stored as calibration checkpoints and mismatches surface in
`unverified_contributions`.

### 4.6 Pet resolution (spec §13-17)

**4.6.1 Pet level (S5).**
`calculated_pet_level = Known(MIN(base + summon_spell_tier, character_level - 1))` when
`base_pet_level` is known (rank rule WIKI_CONFIRMED, quoted). Provenance: NEC = spell
page block/token (WIKI_CONFIRMED; Animate Dead token-only → PARTIALLY_VERIFIED); BST =
Summonedpetpage (WIKI_CONFIRMED); **MAG/ENC/SHM = NULL until override → `PetLevel::
Unknown`** flowing through S5-S13 (review fix): pet stat block renders "level unknown —
enter base level", Stage-B level checks and dual-wield return Unknown-WARN, target-floor
checks WARN. Per-upgraded-level effects (+6% HP/+1 dmg/+5 skills, WIKI_CONFIRMED) apply
in S12; focus-item interaction ships as the off-by-default `pet_focus_rule`
(LEGACY_EQ_DATA, "still needs testing").

**4.6.2 Item validation (S12) — the six spec-§14 stages, all defined** (review fix:
CAN_USE_STATS previously had no rule):

```
CAN_STORE    = pet inventory capacity (base 4 + class bonus, WIKI_CONFIRMED)
CAN_EQUIP    = item.classes ∩ (pet.intrinsic ∪ owner classes) ≠ ∅   # pet_equipment_rule, MAX(id)
               AND slot_legal AND (secondary weapon -> dual_wield check: BST=5 documented,
               others Unknown -> WARN not block)
               AND deity_check = WARN-ONLY ("Needs testing" on Pet Guide; never blocks)
               ; race restrictions not applied to pets (no evidence -> warn-only)
CAN_USE_STATS = CAN_EQUIP
               AND (item.required_level IS NULL OR calculated_pet_level >= item.required_level)
               # verification: NEEDS_INGAME_TEST — the wiki is silent on whether pets
               # respect ITEM required-level (it is explicit only about proc levels).
               # calculated_pet_level Unknown -> CAN_USE_STATS = Unknown (WARN).
               # FALSE -> the item STAYS EQUIPPED ("may wear but not use", spec §14):
               # stats contribute 0, badge INVALID_PET_LEVEL, warning
               # StatsInactivePetLevel{becomes_valid_at: required_level}.
               # This is also the 'compatible with pet level' filter (spec §17) and the
               # ONLY emitter of the INVALID_PET_LEVEL badge.
CAN_ACTIVATE_WORN_EFFECT / CAN_ACTIVATE_PROC / CAN_ACTIVATE_EXALTATION =
               effect.allowed_classes ∩ pet.intrinsic_classes ≠ ∅    # STRICT pool,
                                                                     # PARTIALLY_VERIFIED (spec §14)
               AND calculated_pet_level >= effect.required_level     # WIKI_CONFIRMED
               AND (exaltation) socket.type == exaltation.type
                   AND destination_item_tier >= socket.unlock_tier   # Orn+0..Proc+4
                   AND destination slot allowed                      # 2H -> Primary-only
```

Stage failure below CAN_EQUIP → INVALID_CLASS / INVALID_SLOT / DUAL_WIELD_UNAVAILABLE
badges; CAN_USE_STATS failure → INVALID_PET_LEVEL (item worn, stats 0); effect-stage
failure → EQUIPPABLE_PROC_INACTIVE / EQUIPPABLE_EXALTATION_INACTIVE with the spec §15
string "Equipped successfully; Proc inactive: pet requires level 37"; all pass →
FULLY_ACTIVE. Armor slot conflicts: documented AC-priority rule (higher AC wins; loser
SavedInactive). Weapons: delay ignored, damage used only if > innate.

**4.6.3 Pet buff candidates (S7) — three sources (spec §16):** OWNER_SPELL = available
spells with `pet_targetable=1` (PET always; SINGLE/GROUP per the seeded
NEEDS_INGAME_TEST default — the §2.1 column the review found missing; SELF never) ∪
PET_INNATE_SPELL rows with `minimum_pet_level <= pet level` (Unknown level → included
with WARN) ∪ ITEM_OR_EXALTATION effects that passed Stage B. Summon spells themselves
excluded (slot-text detection, never `spell_type`). Resolution runs the same resolver
with `target=PET, target_level=calculated_pet_level, build_level=character_level`
(PET_INNATE effects use pet level as caster level); disjoint state from the player run;
display split by BuffSource.

### 4.7 Stat assembly (spec §10/§22) and the unknown-formula surface

```
A BASE      race_base_stats + combine(class_stat_mod × 3)      # class_attr_combine=SUM (NEEDS_INGAME_TEST)
B EQUIPMENT class/race/deity/required_level-legal ACTIVE items # illegal -> SavedInactive, 0
C ITEM TIERS bonus(stat,tier) = max(floor(stat*tier/10), bonus(tier-1)+1)   # WIKI_CONFIRMED
D WORN EFFECTS (item-native)                                    # level-gated per effect
E EXALTATION SOCKETS (worn/focus type)                          # SEPARATE StatLine bucket
F BUFFS     resolver output (player S8->S11, pet S9->S13); max-resource vs when-cast vs
            per-tick reported distinctly, never folded together
G CAPS      hard 255; WIS/INT/CHA soft 200 (behavior above soft cap = softcap_model,
            NEEDS_INGAME_TEST, report-only default)
H DERIVED   HP/Mana/AC/resists/skills through the §2.2 formula_table keys
```

Every StatLine records A..F separately (buffed/unbuffed toggle is a rendering choice).
The unknown-formula surface is exactly the §2.2 canonical key table — the engine reads
only those keys, records every `(formula_key, confidence)` consulted during S10-S14 into
`FormulaConfidenceReport`, and quantifies `unverified_contributions` per stat ("HP: 2,340
— 62% of this number uses unverified formulas; edit in Settings → Formulas").

### 4.8 Level-change semantics (spec §18)

The engine NEVER mutates the build; validity is derived per recompute:
`validity(selection) -> Active | SavedInactive{reason, becomes_valid_at}`. Lowering
16→15: items below requirement → SavedInactive red, contribute 0; the selected summon
keeps identity with `valid=false` and the pet block renders greyed-not-blank; a pet
proc at 25 vs pet 22 → "Saved but inactive", becomes_valid_at 25; manual buffs below
level → Rejected{BelowLevel} with the profile row untouched. Raising the level restores
everything because nothing was deleted — there is no invalidation code path to get
wrong. DATA_MISSING (§2.0.4) renders through the same mechanism.

### 4.9 Test plan

Fixtures: SQLite extract of the ~40 probed pages + hand-written rule rows + the shipped
seed overrides (incl. `pet_buff_lines.yaml`) in `eql-engine/tests/fixtures/`.

**Golden stacking cases** (assert full ResolvedBuff rows):

1. **Burnout pet vs player** — Active in the PET set on a NEC WAR/SHD pet; InvalidTarget
   in the PLAYER set (spec §16 separation).
2. **Burnout vs Burnout II** *(restated per review)* — WITH `pet_buff_lines.yaml` seeds
   loaded: PET_HASTE line groups them; optimizer keeps Burnout II, Burnout rejected
   `REPLACED_BY_STRONGER` (line path). A second variant WITHOUT the seeds asserts the
   fallback: both collide on slots 3+4 → `SlotConflict` rejection. Documents that pet
   lines are override-seeded only.
3. **Aegolism threshold rule** — candidate with slot-3 Max Hitpoints < 1100 rejected
   `BlockedByRule`; a ≥1100 candidate passes step 7 (then hits line rules).
4. **Combination buff (CONSUMES_LINE)** — Aegolism consumes HP + AC(1) + AC(4); Spirit
   Armor and a Courage-line buff rejected `ConsumedLineConflict`; component search picks
   Aegolism over the three singles iff its score is higher (both branches, tweaked weights).
5. **Prose rules** — Aegolism "replacing Heroism, Symbol of Marzin and Aegis": Symbol of
   Marzin rejected with **`source_type = 'WIKI_PROSE'`** (canonical literal — review fix)
   and degraded confidence surfaced.
6. **Order-dependent exception** — (MD, FoS) order → both Active; (FoS, MD) → MD rejected.
7. **15-cap with songs counting** — ENC/BRD/NEC, 13 buffs + 4 CONSERVATIVE songs → exactly
   15 Active, 2 lowest rejected `BuffCapReached`; songs and buffs displace each other
   purely by score + tie-breakers.
8. **Instrument scaling** — resonance 28: non-haste components ×2.8 floor; haste and
   mana-regen unmodified; `Required` songs drop without an instrument (BANDOLIER slot
   counts as a source).
9. **Illusion exclusivity** — capacity-1 ILLUSION line; illusions count against the cap.
10. **Target-level floor on a low pet** — L52 buff on pet 22 rejected TargetLevel{40}
    while Active on the player in the same build.
11. **CAN_USE_STATS** *(new)* — item required_level 30 on a level-26 pet: badge
    INVALID_PET_LEVEL, item worn, stats contribute 0, becomes_valid_at 30;
    NEEDS_INGAME_TEST confidence on the rule.
12. **Unknown pet level** *(new)* — ENC animation with NULL base level: PetLevel::Unknown,
    stats banner, Stage-B checks WARN, no crash anywhere in S5-S15.
13. Spec §15 example verbatim — WAR/SHD pet 26, WAR/PAL/RNG/SHD weapon, proc L37 → item
    Active, EQUIPPABLE_PROC_INACTIVE, becomes_valid_at 37.

**Property tests (proptest):** determinism (byte-identical results; permutation-invariant
AUTO sets); level-raise monotonicity (spell set ⊆, optimizer score non-decreasing,
SavedInactive count non-increasing); cap invariants (Active ≤ cap; over_cap ≥ 0;
StatLine components sum to raw_total **including the exaltations bucket**); rejection
completeness (every non-Active has a reason; conflicts carry ids); resolver idempotence
(feeding Active back yields itself); build immutability; randomizer (same seed ⇒
identical build; output always passes the pipeline; slot reroll changes only that slot;
RAID ≥ SCRAPPY expected score over 100 seeds).

**Snapshot tests (insta):** full result JSON for **ENC/BRD/NEC level 15 with the NEC
summon selected** (spec §21 walkthrough — the review fix: the golden fixture uses the
documented NEC base level; the ENC-animation Unknown path is golden 12), SHD/MNK/SHM
level 50 (heavy gear + exaltations), and a MAG pet build (Unknown-level propagation into
formula_confidence). Snapshots pin data/formula versions; conflicts vectors snapshotted
per golden case.

**Benchmarks (criterion, non-gating):** full pipeline < 2 ms cold, < 200 µs warm per
slider tick on the ENC/BRD/NEC fixture; `compare_items` over a 50-candidate page < 5 ms.

---

## 5. App architecture & pages

### 5.1 Stack

| Layer | Pick | Reason |
|---|---|---|
| Shell | Tauri v2 | spec mandate; Rust backend hosts engine + SQLite + scheduler |
| Frontend | Svelte 5 (runes) + TypeScript + Vite | fine-grained reactivity for view-models; solo-dev ergonomics |
| State | Rune `$state` for inputs; **engine result via the `calculation-updated` Tauri event** | inputs edit locally, debounced `recalculate` IPC; every page renders the one immutable result (spec §12) — *no* `$derived computeBuild` in the webview; the engine is Rust (§4.1) |
| Engine | **Rust crate `eql-engine`** (+ `eql-data` loaders) | §4.1 decision; ts-rs generated `.d.ts` types |
| DB access | rusqlite behind Tauri commands; wiki browse/search stays SQL (FTS5 name search) | snapshot loaded once per data_version |
| Importer | Python sidecar (extends `eql_wiki_sync.py`); PyInstaller at M8 | proven fetch/parse/raw-cache pipeline |
| Tests | **proptest + insta + criterion (engine)** · pytest (importer vs cached probe pages) · Vitest (webview view-models only) | §4.9 |

Input flow: `$state` build inputs → debounced `recalculate(build_inputs)` → engine →
`calculation-updated` event → Svelte context at the app root → every page reads the SAME
object. React would also work; the view layer is swappable because zero rules live in it.

### 5.2 Databases, sync, verswho-writes-what

Two files in `%LOCALAPPDATA%/EQLBuilder/` per §2.0.1. Background sync: Python sidecar
orchestrated by a tokio scheduler (Manual default / On-launch 24h / Interval ≥ 6h; keep
0.5 s pacing, never parallel). Run: sidecar `--incremental` → JSON-lines progress on
stdout → Rust forwards `sync-progress` events → parse into `wiki.db.building` (fresh
child rows) → apply `overrides/*.yaml` → **sanity gate** (refuse swap if spells or items
< 90% of previous, or parse-failure > 5%; quarantine banner) → atomic rename →
**post-swap hook inserts the `data_version` row in builds.db** (§2.2 — the writer the
review found missing), sets `app_meta.active_data_version` → `data-updated` event →
snapshot reload → reconciliation pass (§2.0.4: re-resolve soft refs, mark DATA_MISSING)
→ recalc → toast ("14 spells updated, 2 new"). Settings→Formulas saves insert
`formula_version` rows and restamp edited rows. wiki.db rows are never edited directly:
Settings editors for wiki-side data (skill caps, stacking rules, pet base levels) write
**override YAML entries** and trigger an offline recompile — that is how user edits
survive the disposable file.

### 5.3 Global chrome

Persistent top bar: build selector · trio pills · race/deity · **level slider 1-50**
(the ONE global control, spec §1) · buff toggle `○ Unbuffed ● Buffed` + mode OFF/AUTO/
CUSTOM + active-profile dropdown (satisfies spec §3's four placements with one source of
truth) · data pill (data_version, sync status). Left nav: Overview · Spells · Buffs ·
Equipment · Character Stats · Pet · Pet Equipment · Farm List · Settings/Data. Every
stat renders through one `<StatValue>` component (badges, over-cap style, breakdown
tooltip with source revid).

### 5.4 Pages

**Overview** — identity card (race locks Primary only; deity/primary lock at 11 as
info); headline HP/Mana/AC/ATK `unbuffed → buffed (Δ)` with badges; buffs card (active
n/15, top 5 by score, rejected count); pet card (archetype, intrinsic pair, level
formula rendered live `MIN(9+3, 15−1) = 12`, **or "level unknown — enter base level"**
for MAG/ENC/SHM); equipment card (slots filled, red counts); warnings feed; Choose-for-me
button.

**Spells** — one merged row per spell (spec §2/§21): Spell · per-class
`required_class_level` + autogrant (`ENC 12 · NEC 15 (A)`) · Usable ✓/✗ · Target
(TargetNorm incl. **AE**, raw in tooltip) · Type · Duration · Era · **tier stepper 0-10
persisted to `build_spell_tier`** (§2.2 precedence; badge on scaled numbers). Filter bar
exactly spec §21: All / Buffs / Pet Buffs / Songs / Damage / Control / Utility / Pet
Summons — **Damage/Control/Utility read `spell.role`, the APP_DEFINED heuristic (§3.3),
labeled "heuristic" in the filter tooltip**; Songs reads `is_song`; plus era/class
filters, FTS search, "show not-yet-available" toggle. Row expansion: effect slots
verbatim + parsed, stacking rules with confidence, where_to_obtain (→ Farm List
"want this scroll" writes `build_wishlist`), items_with_effect.

**Buffs — four tabs** (spec §11), PLAYER/PET scope switch (separate profiles, spec §4):
Available (columns per spec §11, selection_mode chips) · Active (resolved set + per-buff
contribution + slot count /15; Rejected list with reason + winner + firing rule) ·
Conflicts (node-edge diagram per conflict group + provenance + verified flag — the UI of
`resolved_buff_conflict`) · Custom (checkbox per buff, replace-within-line dropdown,
per-buff tier/caster-level/instrument overrides, bard mode selector, include toggles,
max slots, optimization profile; illegal manual picks shown in red, never silently
accepted; "Save as profile…").

**Equipment** — paper-doll (21 worn slots + **BANDOLIER 1-4 instrument carry slots**,
§2.2); item picker (SQL-backed; **per-row unbuffed/buffed deltas via the
`compare_items` engine command** — review fix); per-item tier stepper +0..+10 (documented
math verbatim) + socket panel (unlock tiers, PARTIALLY_VERIFIED badge on the +4-vs-+10
inconsistency) + Exaltation editor (class-intersection shrink, slot inheritance, reasoned
rejections); right panel breakdown **`Base 85 + Equipment 42 + Item upgrades 8 + Worn
effects 5 + Exaltations 0 + Spell buffs 31 = 171`** — backed 1:1 by StatLine's six
buckets incl. the dedicated `exaltations` field (review fix) — plus cap-waste line `Raw
274 · Effective 255 · Over-cap 19`; compare drawer (both outcomes, unbuffed + buffed +
over-cap deltas, via `compare_items`); buff toggle flips totals without touching
equipment; red saved-but-inactive.

**Character Stats** — 7 attributes, HP, Mana, AC (mit/avoid), ATK, 6 resists (incl.
Void), regen; columns Unbuffed · Buffed · Difference · Cap · Over-cap. Derived numbers
badge and link to their `formula_table` rows. **Assumption banner (corrected per
review): "Stat combination — attribute modifiers: sum of the three classes; HP / mana /
skill caps: best-of-three (unverified)."** Skills section from `skill_cap` breakpoints
(interpolation PARTIALLY_VERIFIED). **Out-of-scope note (§5.6): "Totals exclude
Alternate Advancement, Stance/Invocation, and Ritual effects."**

**Pet** — summon picker (slot-text detection; base level + provenance badge per family;
MAG/ENC/SHM show NEEDS_INGAME_TEST + "enter level" edit link); live level formula;
archetype card (intrinsic pair, innates, inventory slots, dual-wield status); pet stats
Unbuffed · Buffed · Difference (legacy tables collapsed, LEGACY_EQ_DATA, never in
totals); pet buff panel in three groups (Owner / Pet self / Equipment) with independent
resolution and rejected reasons.

**Pet Equipment** — paper-doll with the pet's slot count; pool = intrinsic ∪ owner
(versioned rule); filter rail per spec §17 — **"compatible with pet level" =
CAN_USE_STATS (§4.6.2)**; badges per spec §17 verbatim — **INVALID_PET_LEVEL emitted by
CAN_USE_STATS failure**; per-item **six-stage checklist CAN_STORE → CAN_EQUIP →
CAN_USE_STATS → CAN_ACTIVATE_WORN_EFFECT → CAN_ACTIVATE_PROC → CAN_ACTIVATE_EXALTATION**,
each ✓/✗/? with reason (spec §15's string is the canonical rendering; '?' when pet level
unknown).

**Farm List** — sources: "Add to farm list" everywhere (equipped-but-unacquired via
`build_equipment.is_acquired=0`; anything else — alternative candidates, spell scrolls
(`Spell: X` Itempage rows), spares — via `build_wishlist`, review fix). Item view: want →
mobs (rarity) → zones, mob level range vs character level (red when 10+ above), era
chips; zone "shopping list" aggregation with export; scroll vendors as "Buy: NPC @ zone
(loc)". **Reactivity (review fix): contents = `v_build_farm_list` re-queried on
`calculation-updated`; contents are level-invariant, annotations recompute from
calcResult** (§4.3). Depends on `normalize-mobs` (§3.4b).

**Settings/Data** — sync panel (schedule, run-now, progress, quarantine diff);
**overrides editor** (YAML with schema validation; apply = offline recompile + diff);
**formula tables** (every §2.2 key, inline-editable, verification chip + source + revid,
saves bump formula_version); builds export/import + staleness list; danger zone (rebuild
wiki.db from raw; full re-sync); data-health screen (importer verification histogram +
open import_issues).

### 5.5 Cross-cutting UI rules

1. **Saved-but-inactive = red, never deleted** (spec §18; extends to DATA_MISSING).
2. **Assumption banners** on every affected panel, linking to the controlling formula
   row: bard rotation (mandated wording), stat combination (corrected wording above),
   Kerra stats, MAG pet level, spell-tier scaling.
3. **Verification badges** on every non-verified number via `<StatValue>`, using the six
   canonical statuses (§2.0.2): no glyph VERIFIED_INGAME/WIKI_CONFIRMED · ◆
   MANUAL_OVERRIDE · ◐ PARTIALLY_VERIFIED · ○ NEEDS_INGAME_TEST · ◇ LEGACY_EQ_DATA.
   Tooltip: breakdown, formula key, source page + revid, Settings link.
4. **Rejections always carry reasons** (chips everywhere).
5. **Provenance everywhere** (source_type + revid on every rule-driven display).
6. **Determinism visible** (seed + tie-break order shown in Choose-for-me and AUTO).

### 5.6 Out of scope (explicit — review fix)

**Alternate Advancement, Stances & Invocations, and Rituals are NOT modeled in v2.**
They materially affect real stats, so: (a) the Character Stats and Overview pages carry
a one-line note "excludes AA/Stance effects"; (b) extension points are reserved so
adding them later needs no schema churn — `StatLine` gains buckets by adding fields
(serde-tolerated), `formula_table` keys `aa_*`/`stance_*` are reserved, and
`resolved_buff.source` can gain values by CHECK widening in a builds.db migration.

---

## 6. "Choose for me" — the full algorithm

One canonical design (the engine draft's seeded generator, with the app wizard's
constraints UI merged in — review fix: the two docs described different algorithms).

**RNG & share codes.** PCG32, pinned (never StdRng). Share code
`EQL1-<base36(seed)>-<data_version>-<formula_version>`; version mismatch warns but runs.
Hierarchical substreams `substream(path) = Pcg32::seed(splitmix64(seed ^ fnv1a64(path)))`
with paths like `"classes"`, `"gear/HEAD"`, `"gear/HEAD/attempt2"` — a repair or
single-slot reroll never shifts any other decision.

**Wizard (UI).** Step 1 constraints: level (current | random), **budget tier
SCRAPPY / DUNGEON / RAID** (review fix: the budget knob is now in the wizard), era
ceiling, must-include classes/race, pet required?, include-unlockable-races toggle,
"aspirational gear" toggle (RAID only). Step 2 generate → review screen with per-section
lock + reroll → Accept materializes a normal, fully editable build. Everything flows
through the same engine and validators — the generator sits in front of the pipeline,
never a second rules implementation.

**Generation order** (each step consumes only its substream):

```
1 CLASSES  3 distinct of 16, uniform over C(16,3) (honoring must-includes)
2 RACE     pool = races whose Primary list intersects the drawn classes; the drawn race
           DESIGNATES its legal class as Primary in the same draw (race restricts ONLY
           Primary — WIKI_CONFIRMED); include_unlockable per toggle
3 DEITY    uniform over all 17 including Agnostic.  (REVIEW FIX: no race/class↔deity
           legality table exists — the seeded-legality reference is DROPPED until
           backlog V17 provides data; deity still affects item legality downstream)
4 LEVEL    current build level (default) | random 1-50
5 PET      if any class ∈ {MAG, NEC, ENC, SHM, BST, SHD}: pick a summoner class;
           summon = highest available at level with p = {SCRAPPY .5, DUNGEON .75, RAID .95},
           else uniform over lower; tier ~ budget distribution.
           If the family's base level is NULL (MAG/ENC/SHM without override), the build
           still generates; the pet block carries PetLevel::Unknown per §4.6.1
6 GEAR     per slot (fixed order; 2H-vs-1H first): pool = items legal for
           (classes, race, deity, level, slot);
           weight(item) = w_budget(era, source) × w_rarity(drop text) × w_fit
           where w_fit = level-proximity × optimization-profile stat alignment
           (REVIEW FIX: the app draft's profile-scoring weight folds INTO w_fit as a
           multiplicative factor; the budget knob governs everything else);
           empty-slot probability per budget
7 TIERS    per item ~ budget distribution
8 EXALTATIONS  per socket unlocked by the item's tier: fill with p_budget from the legal
           pool (class overlap ≠ ∅ AND post-insert intersection still covers ≥1 selected
           class [documented shrink rule] AND slot inheritance OK AND (pet items)
           intrinsic-class match)
9 SPELL TIERS  spell_tier_default ~ budget; per-spell jitter ±1 clamped 0..10
10 BUFFS   buff_mode = AUTO; optimization_profile weighted (BALANCED .4, rest uniform);
           pet profile only if pet exists; bard_maintenance = CONSERVATIVE if BRD
```

**Budget tiers:**

| Knob | SCRAPPY | DUNGEON | RAID |
|---|---|---|---|
| Gear pool bias | vendor/common, no raid eras | named-mob drops in level range | raids/epics/latest era boosted |
| Rarity weighting | Common×4, Rare×1, skip Ultra | Common×1, Rare×2 | Rare×2, Ultra×3 |
| Item tier dist | {0:.6, 1:.3, 2:.1} | triangular 2-5 | triangular 5-10 |
| Exaltation fill p | .1 (weapons only) | .4 | .9 |
| Spell tier default | 0-1 | 2-4 | 5-10 |
| Empty-slot p | .35 | .1 | 0 |

**Repair.** Generate-then-validate with local deterministic repair, never global
restart: per violation in stable slot order, redraw ≤3 attempts from
`path + "/attemptK"` with the violated constraint added to the filter; then degrade
deterministically (exaltation → drop; item → lower tier pool → empty; pet tier → reduce
until legal). SavedInactive-class level gates allowed only under RAID + "aspirational"
toggle. Output always passes the pipeline clean (test-asserted, §4.9).

---

## 7. Farm list — data model + page

**Data model.** Wants live in two builds.db tables: `build_equipment` rows with
`is_acquired=0` (slot-bound wants) and `build_wishlist` (free wants: alternative
candidates for one slot, `Spell: X` scroll items, pet spares — review fix). Sources
resolve through wiki.db: `drops` (name-keyed, rarity) → `mobs` (normalized
`level_min/max` from `normalize-mobs`, §3.4b) → `mob_zone` → `zone` (era, "Level of
Monsters" range) + `zone_unique_item`; all joined by the `v_item_sources` view and
surfaced per build by the cross-db TEMP view `v_build_farm_list` (§2.3).

**Page.** Item view: want → dropping mobs (rarity chips) → zones; columns mob level
range (red when 10+ above character level), zone, era chip, respawn. Zone view
("shopping list"): wants grouped by zone — "Lower Guk (Classic, mobs 30-40): 3 wanted
items — [item ← mob (Rare)] …" — sorted by want count then level proximity; copy/export
as text. Era filter across the page. Spell scrolls: vendor sources render "Buy: NPC @
zone (loc)" from `spell_source`. Acquisition: checking "acquired" flips
`is_acquired`/removes the wishlist row.

**Reactivity contract (review fix).** Contents re-query on every `calculation-updated`
event; the want SET is level-invariant; level-relative coloring and inactive-want
flags recompute from `calcResult`. The farm list is not an engine stage (§4.3).

---

## 8. Verification backlog & override workflow

### 8.1 How confirming a fact updates the database

1. Test the fact in-game (each item below says exactly what to measure).
2. Record it in an override YAML entry (or, for formula keys, directly in
   Settings→Formulas): set the value, `verified: true`,
   `tested: {by: dev, date: ..., method: in_game}`.
3. `apply-overrides` (auto after next sync, or "apply" in Settings) stamps the target
   rows `verification_status='VERIFIED_INGAME'`, logs to `override_application`
   (wiki revid → file → row), and formula edits insert a `formula_version` row.
4. The engine's fingerprints include FT/DV versions → affected stages recompute; badges
   disappear; `unverified_contributions` shrinks. Re-syncs can never clobber the value
   (override re-applied last; drift surfaces as WIKI_CHANGED_UNDER_OVERRIDE).

### 8.2 The checklist (every PARTIALLY_VERIFIED / NEEDS_INGAME_TEST fact)

| ID | Fact to verify | Where it lives | How to test in beta | Status today |
|---|---|---|---|---|
| V1 | Base pet level per MAG/ENC/SHM summon spell (per rank R1/R2/...) | `spell_pet_summon.base_pet_level` (override `base_levels`) | summon each pet at tier 0, `/pet leader` + con color / combat log level | NEEDS_INGAME_TEST (NULL) — blocks nothing; pet shows "level unknown" |
| V2 | Dual-wield unlock pet level, 8 of 9 families | `pet_archetype.dual_wield_unlock_pet_level` | give two 1H weapons, level pet up, watch for offhand swings | NEEDS_INGAME_TEST (BST=5 confirmed) |
| V3 | Do pets respect item Deity restrictions? | `pet_equipment_rule.respect_deity_restrictions` | equip a deity-locked item on a pet | NEEDS_INGAME_TEST (Pet Guide itself asks) |
| V4 | Can ordinary Single/Group beneficial buffs land on pets (per spell)? | `spell_target_rule.pet_targetable` | cast representative Single + Group buffs on own pet | NEEDS_INGAME_TEST (seeded default: yes) |
| V5 | Do pets respect ITEM required_level (CAN_USE_STATS)? | engine rule §4.6.2; toggleable via override | equip Ragebringer-class (req 46) item on a low pet; check stats | NEEDS_INGAME_TEST (assumed yes) |
| V6 | Spell upgrade tier 0-10 effect scaling | `formula_table['spell_tier_scaling']` | compare one spell's tooltip/landed value at two tiers | NEEDS_INGAME_TEST (zero wiki presence) |
| V7 | Instrument resonance→multiplier curve (0..28) | `formula_table['instrument_multiplier']` 29 rows | one song, several instruments of known resonance, measure effect | NEEDS_INGAME_TEST (anchor 28⇒280% confirmed) |
| V8 | Multiclass attribute-modifier combination (SUM?) | `formula_table['class_attr_combine']` | create trio, compare char sheet vs race base + summed mods | NEEDS_INGAME_TEST |
| V9 | Multiclass HP/mana/skill combination (BEST_OF?) | `formula_table['multi_class_*_combine']` | same trio, compare HP vs per-class predictions | NEEDS_INGAME_TEST |
| V10 | Base HP and STA→HP per level (anchors at L50/L60 only) | `formula_table['base_hp'/'hp_per_sta']` | record HP at several levels with known STA | PARTIALLY_VERIFIED anchors |
| V11 | Mana per WIS/INT + base mana | `formula_table['base_mana'/'mana_per_stat']` | record mana across levels/stat values | PARTIALLY_VERIFIED (~11/pt @60) |
| V12 | AC softcap + resist conversion in EQL | `formula_table['ac_softcap'/'resist_percent']` | controlled melee/resist logging | LEGACY_EQ_DATA |
| V13 | Exaltation socket unlock tiers ("+4 fully upgraded" vs 0-10 scale) | `exaltation_socket_rule` | upgrade one item past +4, check socket panel | PARTIALLY_VERIFIED |
| V14 | Transferred proc/Exaltation CLASS activation on pets (intrinsic-only vs full pool) | `pet_equipment_rule.effect_class_source` | transfer a class-locked proc to a pet-wearable item | PARTIALLY_VERIFIED (spec §14) |
| V15 | Behavior above the 200 WIS/INT/CHA soft cap | `formula_table['softcap_model']` | measure mana gain per point above 200 | NEEDS_INGAME_TEST |
| V16 | Caster-level interpolation between (Lx)..(Ly) endpoints; duration ditto | `formula_table['effect_caster_level_interpolation']`, `spell_effect.caster_level_scaling` | one scaling buff at 3+ caster levels | NEEDS_INGAME_TEST (linear assumed) |
| V17 | Race/class↔deity legality matrix | (no table yet — randomizer draws uniform; add `race_class_deity` when data exists) | character creation screens per race/primary | ABSENT — nobody has this data; collect during play |
| V18 | Pet buff-line memberships (PET_HASTE/PET_AC/PET_STRENGTH...) | `overrides/seeds/pet_buff_lines.yaml` | stack candidate pet buffs pairwise, note overwrites/blocks | NEEDS_INGAME_TEST (seed = Burnout family only) |
| V19 | Pet buff window size (15 like players?) | `formula_table['pet_buff_slot_limit']` | stack >15 pet buffs | NEEDS_INGAME_TEST |
| V20 | Buff Lines page corrections (missing item buffs, "+0 rows", footnote exceptions, Focus-of-Spirit order rule) | `buff_line*`, `spell_stacking_rule` via overrides | pairwise stacking tests per import_issue queue | NEEDS_INGAME_TEST (page self-declared incomplete) |
| V21 | Kerra base stats ("assumed from Vah Shir") | `race_base_stats` row via override | roll a Kerra, read the character sheet | PARTIALLY_VERIFIED |
| V22 | Iksar/Troll regen + racial ability numbers (page flagged "incorrect for EQL") | `race_ability` | measure regen ticks | PARTIALLY_VERIFIED |
| V23 | Skill-cap curves between breakpoints | `skill_cap` breakpoints via override | note skill cap at several levels for 2-3 skills | PARTIALLY_VERIFIED |
| V24 | Pet innate spell lists + minimum pet levels (non-BST) | `pet_innate_spell` via override | observe pet self-casts while leveling | PARTIALLY_VERIFIED (BST structured) |
| V25 | Pet focus items vs upgraded pet levels ("still needs testing" on wiki) | `formula_table['pet_focus_rule']` | summon with/without focus item | LEGACY_EQ_DATA, off by default |
| V26 | Bard song sustainable counts / minimum cycle times | `bard_song_rule.is_sustainable/minimum_cycle_time` | practical twisting test | MANUAL_OVERRIDE defaults |
| V27 | Mote drop-level gating tiers 1-3 ("need confirmation" on wiki) | `formula_table['item_tier_xp_cost']` notes | farm motes at known levels | PARTIALLY_VERIFIED |
| V28 | Era for the ~44% of spells defaulted to Classic | `spell.era` (`era_source='DEFAULT'` list in run report) | cross-check vendor zones / patch notes | PARTIALLY_VERIFIED |

The app's data-health screen renders this list live from the verification histogram +
open `import_issue` rows; the M8 deliverable ships it in-app with per-item "record
result" shortcuts that write the override entry.

---

## 9. Roadmap M0-M8

Sequencing honors the spec's stated priority ("the next structural priority should be
the spell-effect parser and stacking-rule engine"). **Schema ownership (review fix):
`docs/schema.sql` generated from §2 is the single DDL source; M0's importer executes the
wiki.db section (so M0 needs no app), M1's app migration executes the builds.db section.**

| # | Milestone | Depends | Definition of done |
|---|---|---|---|
| **M0** | **Spell & effect importer** (Python). wiki.db DDL (§2.1) executed by the importer; both spell templates + `SpellSlotRow(Smart)` + `SpellWhereTable` + `{{Summonedpetpage}}`; effect grammar incl. `Stacking:` rows; named static pages (Buff Lines seed, Pet Guide, Statistics, Game Mechanics, Item Upgrade System, Exaltations, Skills, Deities, races, zones); `reparse-items` (+canonicalization map §3.4); `normalize-mobs` (§3.4b); overrides loader + `check-overrides`; shipped seeds incl. `pet_buff_lines.yaml`; child pre-delete; case dedup; total dispatch; run report | — | wiki.db ≥1,900 spells with class levels/effects/stacking; Aegolism's block row parses structured; NEC pet blocks + BST pet pages land; Buff Lines members `verified=0`+revid; **all inserts satisfy the §2.1 CHECKs (incl. AE/UNKNOWN targets, UNKNOWN duration, canonical verification enum)**; ≥95% mobs get numeric level ranges; pytest fixtures green on all cached probe pages; full rebuild offline from raw/; parse-failure < 2% |
| **M1** | **builds.db + plumbing.** App migration runs §2.2 DDL (incl. `build_spell_tier`, `build_wishlist`, `formula_table` seeds, `pet_equipment_rule` row 1, fv1/dv1 rows); wiki.db atomic swap + **post-swap `data_version` writer** + reconciliation pass (§2.0.4); Tauri v2 + Svelte shell; `get_snapshot`/`query_items`; top bar wired to a stub `recalculate` | M0 | create/save/reopen a build; both DBs in %LOCALAPPDATA%; swap while app open → reload event → soft refs re-resolved (a renamed spell reconciles by name_canonical; a deleted one goes DATA_MISSING red, not lost) |
| **M2** | **Rust engine, golden-tested.** `eql-engine` + `eql-data`: S1-S15, 12-step resolver, optimizer + tie-breakers, buff lines/combination buffs, effect scaling, caps/over-cap, formula consumption, PetLevel::Unknown path, trace output, confidence report; `calculation-updated` event; ts-rs types | M0, M1 | golden suite green (§4.9 cases 1-13 incl. the seeded-vs-unseeded Burnout pair, WIKI_PROSE literal, CAN_USE_STATS, Unknown pet level); proptest + insta suites green; determinism byte-identical; **crate has no I/O and no Tauri/webview imports**; criterion budget met (<2 ms cold) |
| **M3** | **Core pages.** Overview, Spells (role filters + tier stepper → `build_spell_tier`), Buffs (4 tabs, PLAYER/PET, profiles, bard modes), Character Stats (+AA out-of-scope note, corrected assumption banner); shared `<StatValue>`; minimal Settings→Formulas (+`formula_version` writer) | M2 | spec §21 build browsable end-to-end; merged spell rows; AUTO resolves with reasons + provenance; CUSTOM persists per profile; every unverified number badged and editable |
| **M4** | **Equipment depth.** Paper-doll + BANDOLIER slots; picker + compare drawer via `compare_items`; tier math; sockets + Exaltation editor; red inactive | M2-M3 | spec §10 breakdown format exact (six buckets incl. Exaltations); compare shows both outcomes + cap waste; tier math passes unit tests vs Item Upgrade System tables; illegal Exaltation insert shows reasoned rejection; buff toggle flips totals without mutating equipment |
| **M5** | **Pet system.** Pet + Pet Equipment pages; six-stage validation incl. CAN_USE_STATS; pet buff resolution vs pet level; Unknown-level UX; editable pet archetype data via overrides | M2-M4 | spec §15 example verbatim; Burnout PET-set-only; all §17 badges reachable in tests (**incl. INVALID_PET_LEVEL via CAN_USE_STATS**); ENC animation renders the Unknown-level flow without errors; base-level provenance visible |
| **M6** | **Farm list + background sync.** Farm List page over `v_build_farm_list` (wishlist + equipment wants, zone aggregation, export); scheduler + sidecar + progress UI + sanity gate + swap + auto-recalc | M0-M1 (sync), M3 (UI) | add 5 items (incl. one scroll + one wishlist alternative) → correct zone shopping list; live wiki edit round-trips: fetch → gate → swap → data_version row → reconcile → toast → recalc, no restart |
| **M7** | **Choose-for-me.** §6 wizard incl. budget tiers; seeded generator + repair; per-section lock/reroll | M2-M5 | 1,000 seeded runs, zero validation errors; same seed ⇒ identical build; slot reroll changes only that slot; generated builds fully editable |
| **M8** | **Polish + packaging.** PyInstaller sidecar (or Rust port decision gate — not earlier); MSI/NSIS; first-run onboarding (seed wiki.db from mirror copy or fresh sync); builds.db backup; perf pass (slider ≥30 fps); **in-app verification checklist (§8.2) with "record result" shortcuts** | all | clean install without Python renders the §21 build after first sync; no handles in OneDrive ever; upgrade preserves builds.db; checklist shipped |

**Blocked-on-in-game-verification callout:** M2 (multiclass combination, spell-tier
scaling), M5 (MAG/ENC/SHM base levels, dual-wield, transferred-effect class edges). None
block shipping — each is an editable, badged formula/override row with §8.2 telling the
user exactly what to measure.

### 9.1 Risk register

| # | Risk | L/I | Mitigation |
|---|---|---|---|
| 1 | Wiki template drift (`Spellpagesmart` "under construction") | H/H | raw wikitext kept per page; >5% parse-failure sanity gate blocks swap; pytest fixtures on cached probes; unknown-param logging; versioned parser re-runs old dumps |
| 2 | Beta data churn (July-7 pet rework happened mid-recon) | H/M | incremental sync + per-row revids; builds pinned to data_version with staleness badges + auto-recalc; per-row verification; per-sync diff |
| 3 | Stacking data incompleteness (Buff Lines self-declared; pet lines nonexistent) | H/H | seed `verified=0` + curated overrides win; Conflicts tab exposes every rule + provenance; persistable trace; golden tests lock documented cases; pet lines explicitly seed-only (golden 2 covers the unseeded fallback) |
| 4 | OneDrive/SQLite corruption | M/H | app never opens a db under OneDrive; atomic temp-build-then-rename; WAL single-writer; builds.db export |
| 5 | Formula unknowns destroy trust | certain/M | no hidden constants — every unknown an editable badged row; documented anchors seed defaults; §8.2 checklist; formula_version audit trail |
| 6 | Stacking-engine correctness | M/H | pure Rust function, golden+property suite as the M2 gate; deterministic tie-breaks; per-decision rule ids; exact-but-scoped optimizer (per-line + small component search, never SAT) |
| 7 | Normalization debt (free-text levels, dirty tokens, typo keys, case dupes) | H/M | §3.4 canonicalization map + coverage report; `normalize-mobs` ≥95% gate; pre-delete children; name_canonical UNIQUE dedup; unparseable renders "?" never 0 |
| 8 | Python sidecar packaging | M/M | keep Python through M7; M8 decision gate (PyInstaller vs Rust port validated by the same pytest corpus); app degrades gracefully (sync off, wiki.db still rebuildable) |
| 9 | Cross-db soft references dangle after dedup/renames | M/M | §2.0.4 reconciliation (pageid → name_canonical → DATA_MISSING); dedup deletions safe by construction; reconcile step in the post-swap hook, tested in M1 DoD |
| 10 | Engine/webview type drift | L/M | ts-rs generated types only; no hand-written mirrors; CI compiles both |
