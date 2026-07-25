# Building from source

A Tauri v2 desktop app: Rust workspace + Svelte 5 frontend, SQLite for data. Windows is the
only target that currently ships, though the code is not deliberately Windows-only.

## Prerequisites

- **Rust** (stable) — <https://rustup.rs>
- **Node.js** 18+ and npm
- **Tauri v2 system dependencies** — follow
  <https://v2.tauri.app/start/prerequisites/> for your OS. On Windows this means the
  **Microsoft C++ Build Tools** and **WebView2** (already present on Windows 11).
- **Python 3.9+** — only needed to build the game database (below).

## Run it

```bash
cd app
npm install
npm run tauri dev
```

First launch needs a database — see below, or the app will start with an empty catalog.

## Checks

```bash
cd app
cargo test -p eql-engine -p eql-data --lib   # the gate CI runs; must stay green
npm run check                                # svelte-check; must report 0 errors
```

`--lib` runs the pure engine unit tests, which need nothing but the source. A bare
`cargo test` also runs the integration tests under `tests/`, and those read a locally
built game database plus fixture exports — neither is redistributed here, so they only
pass once you've generated the database (below). That's expected, not a broken checkout.

## The game database

The app reads two SQLite databases:

| DB | What | Where |
|---|---|---|
| `wiki.db` | game data mirror — **disposable, rebuildable** | bundled with the app; installs to `%LOCALAPPDATA%/EQLBuilder` |
| `builds.db` | your saved characters — **precious** | `%LOCALAPPDATA%/EQLBuilder`, created on first run |

**`wiki.db` is not in this repository.** It is derived from the game's own files and from
community sites, and this project does not redistribute that data (see [NOTICE.md](NOTICE.md)).
You have two ways to get one:

### Option 1 — download the dev-assets bundle (easiest)

Grab `dev-assets.zip` from the [releases page](../../releases). It contains a prebuilt
`wiki.db` plus the icon set. Unzip:

- `wiki.db` → `app/src-tauri/resources/wiki.db`
- `icons/` → `app/public/icons/`

### Option 2 — build it yourself

```bash
python scripts/eql_wiki_sync.py        # mirror the community wiki -> db/eql.db
python scripts/import_supplemental.py  # non-canonical seed entries (see CONTRIBUTING.md)
python scripts/mark_epic_items.py      # tag the class epic weapons (optimizer opt-in)
python scripts/make_dist_db.py         # slim db/eql.db -> app/src-tauri/resources/wiki.db
python scripts/fetch_icons.py          # icon set -> app/public/icons/
```

Other importers under `scripts/` fill in derived data (stats, spells, AA, pet estimates). Read
the header comment in each; several take an env var pointing at your own game install rather
than a hard-coded path. Be polite to the community sites — the sync script is rate-limited on
purpose; don't remove that.

**After rebuilding the database, copy it over the live copy** at
`%LOCALAPPDATA%/EQLBuilder/wiki.db` and touch `app/src-tauri/src/main.rs`, or the running app
will keep reading the stale one. This trips everybody once.

## Release build

```bash
powershell -File scripts/build_release.ps1
```

Use the script, not a bare `tauri build`. It sets `--remap-path-prefix` so absolute build paths
from your machine don't get baked into the binary — rustc embeds them in panic messages and
`strip` does not remove them, so a bare build leaks `C:\Users\<you>\...` into the shipped exe.

If you cut your own release, scan the resulting binary for your own username and paths before
distributing it. (The maintainer's scanner isn't in this repo: it matches against a private list
of personal terms, which would defeat the purpose if published.)
