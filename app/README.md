# EQL Character Builder — app (M1 scaffold)

Tauri v2 + Svelte 5 desktop app that reads the wiki mirror (`../db/eql.db`) and plans a
triple-class EverQuest Legends character. This is the **M1 scaffold** from
`../docs/character-builder-plan.md` — it compiles and runs, showing wearable gear +
transferable mods for a chosen class set. Engine rules land in M2+.

## Architecture (plan §4.1 — enforced)

```
app/
├─ crates/eql-data     Shared serde types (Item, BuffLine, BuildCalculationResult, Verification)
├─ crates/eql-engine   PURE calc crate — NO I/O, NO Tauri imports. Deterministic. Unit-tested.
├─ src-tauri           Tauri shell: SQLite access (rusqlite) + commands query_items/get_snapshot
├─ migrations/builds.sql   builds.db DDL (the precious DB; %LOCALAPPDATA%, never OneDrive)
└─ src/                Svelte webview — renders results, contains ZERO game rules
```

The engine never imports Tauri or a database driver; the Tauri layer builds a `Snapshot`
from SQLite and hands it in. That keeps the rules deterministic and golden-testable (M2).

## Prerequisites

- Rust (stable) + Cargo
- Node 18+ and npm
- Tauri v2 CLI: `cargo install tauri-cli --version "^2"` (or use `npx @tauri-apps/cli`)
- A built wiki DB at `../db/eql.db` (run `python ../scripts/eql_wiki_sync.py sync` first).
  On first run the app also looks in `%LOCALAPPDATA%/EQLBuilder/wiki.db`.

## Run (dev)

```
cd app
npm install
cargo tauri dev        # builds crates, launches Vite + the window
```

Test the pure engine on its own (fast, no toolchain beyond Rust):

```
cargo test -p eql-engine
```

## What works now (M1)

- Pick any of the 16 classes (defaults to SHD/MNK/SHM); the window shows every wearable
  item and the distinct transferable worn/focus mods, recomputed on each change.
- `query_items(classes)` and `get_snapshot(classes)` Tauri commands over `db/eql.db`.
- `builds.sql` creates the precious builds.db with versioning, the editable `formula_table`
  (seeded multiclass-combine + spell-tier defaults, all NEEDS_INGAME_TEST), soft-ref build
  tables, and the pet-equipment rule row.

## Next milestones (see plan §9)

- **M2** — `eql-engine`: 12-step stacking resolver (consumes `buff_line`/`buff_line_member`),
  optimizer, effect scaling, pet resolution, golden + property tests.
- **M3** — Spells / Buffs / Character Stats pages; `build_spell_tier` stepper.
- **M4** — paperdoll + tier math + Exaltation sockets.
- **M5** — pet system + CAN_USE_STATS validation.

## Notes

- Never open a DB under OneDrive (plan risk 4): the app uses `%LOCALAPPDATA%/EQLBuilder`.
  The `../db/eql.db` fallback is a dev convenience only.
- Types cross the boundary via `eql-data`; add `ts-rs` in M2 to generate `src/lib/api.ts`
  instead of hand-maintaining it.
