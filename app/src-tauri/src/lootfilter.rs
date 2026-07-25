//! Read/write the game's AdvLoot personal loot-filter file, and maintain the catalog of
//! real game item ids that powers the "add items" picker.
//!
//! File: `<EQL>/userdata/LF_<Char>_<city>.ini`
//! Line: `GAMEITEMID^FILTER^ICON^Name +N`   (header `#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME`)
//!
//! Two hard facts drive the whole design (see memory `eql-loot-filter-format`):
//!   * the GAME item id (col 0) is what the game matches on, and it is TIER-INDEPENDENT —
//!     "Keg Mallet" is 177815 whether it dropped at +4 or +7 — so ONE entry covers every
//!     tier and the "+N" in the name is a cosmetic snapshot;
//!   * that game id is NOT our wiki pageid, and nothing shipped in the client maps the two
//!     (dbstr_us.txt has zero item names). Real ids therefore exist only in files the user
//!     already has: existing `LF_*.ini` filters and `*-Inventory.txt` dumps. We accumulate a
//!     persistent (id, name, icon) catalog from those so the picker can offer real ids.
//!
//! FILTER_ID is the per-item action, one per item — the columns of the in-game Edit Loot
//! Filters window, in order: 1 = Loot, 2 = Merge, 3 = Store, 4 = Sell. Merge/Store/Sell all
//! loot the item first, then act (the chat log shows a Sell-filtered drop "looted ... and sold
//! it"). Confirmed against a live filter: motes = 2 (Merge fodder), named gear = 3 (Store),
//! Rusty/Fine Steel trash = 4 (Sell).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eql_data::Item;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::inventory::{guess_eql_dir, norm, split_tier};

/// One line of a loot-filter file, plus read-path enrichment (ignored when writing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LfEntry {
    pub item_id: i64,   // GAME item id — the field the game matches loot against
    pub filter_id: i64, // 1 Loot / 2 Merge / 3 Store / 4 Sell (kept raw so unknown values round-trip)
    pub icon_id: i64,
    pub name: String, // full name as written, may carry a cosmetic " +N"
    /// name minus the "+N" (enrichment; empty on the write path)
    #[serde(default)]
    pub base_name: String,
    /// tier read from the "+N", 0 if none (enrichment)
    #[serde(default)]
    pub tier: u32,
    /// wiki pageid matched by base name, for stat/tooltip display (enrichment)
    #[serde(default)]
    pub pageid: Option<i64>,
}

/// Parse a loot-filter file's text into entries. Blank lines and the `#` header are skipped;
/// a line with fewer than 4 caret fields or a non-numeric item id is dropped (never guessed).
pub fn parse_lf(text: &str) -> Vec<LfEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('^').collect();
        if cols.len() < 4 {
            continue;
        }
        let Ok(item_id) = cols[0].trim().parse::<i64>() else {
            continue;
        };
        let filter_id = cols[1].trim().parse::<i64>().unwrap_or(0);
        let icon_id = cols[2].trim().parse::<i64>().unwrap_or(0);
        // names shouldn't contain '^', but if one did, keep everything after field 3
        let name = cols[3..].join("^").trim().to_string();
        let (base_name, tier) = split_tier(&name);
        out.push(LfEntry {
            item_id,
            filter_id,
            icon_id,
            name,
            base_name,
            tier,
            pageid: None,
        });
    }
    out
}

/// Serialize entries back to the exact game file format (header + one line each).
pub fn serialize_lf(entries: &[LfEntry]) -> String {
    let mut s = String::from("#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME\n");
    for e in entries {
        s.push_str(&format!("{}^{}^{}^{}\n", e.item_id, e.filter_id, e.icon_id, e.name));
    }
    s
}

/// `LF_Testchar_qeynos.ini` -> ("Testchar", "qeynos"). Character names are single
/// words, so the first underscore splits name from city; the city keeps any remainder.
fn parse_lf_filename(name: &str) -> Option<(String, String)> {
    let stem = name
        .strip_suffix(".ini")
        .or_else(|| name.strip_suffix(".INI"))
        .unwrap_or(name);
    let rest = stem.strip_prefix("LF_").or_else(|| stem.strip_prefix("lf_"))?;
    let (chr, city) = rest.split_once('_')?;
    (!chr.is_empty() && !city.is_empty()).then(|| (chr.to_string(), city.to_string()))
}

/// The `<EQL>/userdata` folder where the game keeps LF_*.ini (None if EQL isn't found).
fn userdata_dir() -> Option<PathBuf> {
    let d = guess_eql_dir()?.join("userdata");
    d.is_dir().then_some(d)
}

// --------------------------------------------------------------- list / read / write

/// One LF_*.ini the app found.
#[derive(Debug, Clone, Serialize)]
pub struct LfFile {
    pub path: String,
    pub name: String,
    pub character: Option<String>,
    pub city: Option<String>,
    pub entry_count: usize,
    pub modified_epoch: u64,
}

/// What `lf_list_files` reports: the userdata folder scanned and the LF files in it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LfScan {
    pub dir: Option<String>,
    pub files: Vec<LfFile>,
}

/// List `LF_*.ini` in the game's userdata folder, newest first.
pub fn list_files() -> LfScan {
    let Some(dir) = userdata_dir() else {
        return LfScan::default();
    };
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !(lower.starts_with("lf_") && lower.ends_with(".ini")) {
                continue;
            }
            let path = entry.path();
            let entry_count = std::fs::read_to_string(&path)
                .map(|t| parse_lf(&t).len())
                .unwrap_or(0);
            let modified_epoch = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let (character, city) = match parse_lf_filename(&name) {
                Some((c, t)) => (Some(c), Some(t)),
                None => (None, None),
            };
            files.push(LfFile { path: path.display().to_string(), name, character, city, entry_count, modified_epoch });
        }
    }
    files.sort_by(|a, b| b.modified_epoch.cmp(&a.modified_epoch).then(a.name.cmp(&b.name)));
    LfScan { dir: Some(dir.display().to_string()), files }
}

/// A parsed LF file ready for the editor.
#[derive(Debug, Clone, Serialize)]
pub struct LfDoc {
    pub path: String,
    pub character: Option<String>,
    pub city: Option<String>,
    pub entries: Vec<LfEntry>,
}

/// Read + parse an LF file, enrich each entry with a wiki pageid (by base name), and harvest
/// its real game ids into the catalog (reading a real filter grows what the picker can offer).
pub fn read_file(path: &Path, items: &BTreeMap<i64, Item>) -> std::io::Result<LfDoc> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut entries = parse_lf(&text);

    let by_name = name_index(items);
    for e in &mut entries {
        e.pageid = by_name.get(&norm(&e.base_name)).and_then(|v| v.first().copied());
    }
    // harvest every entry (id, base name, icon) into the catalog
    let rows: Vec<(i64, String, Option<i64>)> = entries
        .iter()
        .map(|e| (e.item_id, e.base_name.clone(), (e.icon_id != 0).then_some(e.icon_id)))
        .collect();
    let _ = catalog_upsert(&rows, "lf", items);

    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let (character, city) = match parse_lf_filename(&name) {
        Some((c, t)) => (Some(c), Some(t)),
        None => (None, None),
    };
    Ok(LfDoc { path: path.display().to_string(), character, city, entries })
}

/// Write entries to `<userdata>/LF_<char>_<city>.ini`, backing up any existing file to
/// `<name>.bak` first. Returns the path written. `dir_override` lets a caller (or a test)
/// target a specific folder; otherwise the game's userdata folder is used.
pub fn write_file(
    character: &str,
    city: &str,
    entries: &[LfEntry],
    dir_override: Option<&Path>,
) -> std::io::Result<String> {
    let dir = match dir_override {
        Some(d) => d.to_path_buf(),
        None => userdata_dir().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "EQL userdata folder not found — set the game folder first",
            )
        })?,
    };
    let chr = sanitize_token(character);
    let cty = sanitize_token(city);
    if chr.is_empty() || cty.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "character and city are required for the file name",
        ));
    }
    let path = dir.join(format!("LF_{chr}_{cty}.ini"));
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("ini.bak"));
    }
    std::fs::write(&path, serialize_lf(entries))?;
    Ok(path.display().to_string())
}

/// Keep a file-name token to the characters the game uses (letters/digits); a stray space or
/// separator would break the `LF_<char>_<city>` parse on the next read.
fn sanitize_token(s: &str) -> String {
    s.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

// ----------------------------------------------------------------- known-item catalog

/// name -> [pageid…] (lowest id first), the same bridge inventory.rs builds.
fn name_index(items: &BTreeMap<i64, Item>) -> BTreeMap<String, Vec<i64>> {
    let mut by_name: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for it in items.values() {
        by_name.entry(norm(&it.name)).or_default().push(it.pageid);
    }
    for v in by_name.values_mut() {
        v.sort_unstable();
    }
    by_name
}

/// Upsert (game_item_id, base_name, icon) rows into the catalog. Missing icon/pageid are
/// filled from the wiki by name. Returns how many rows were written. A newer row's real icon
/// replaces an older NULL, but we never blank a known icon back out.
pub fn catalog_upsert(
    rows: &[(i64, String, Option<i64>)],
    source: &str,
    items: &BTreeMap<i64, Item>,
) -> rusqlite::Result<usize> {
    let by_name = name_index(items);
    let mut c = crate::builds::conn()?;
    let tx = c.transaction()?;
    let mut n = 0usize;
    for (id, base, icon) in rows {
        if *id <= 0 || base.is_empty() {
            continue;
        }
        let key = norm(base);
        let pageid = by_name.get(&key).and_then(|v| v.first().copied());
        let icon = icon.or_else(|| pageid.and_then(|p| items.get(&p)).and_then(|it| it.icon_id));
        tx.execute(
            "INSERT INTO known_game_item(game_item_id,name,name_key,icon_id,pageid,source,updated)
               VALUES(?1,?2,?3,?4,?5,?6,datetime('now'))
             ON CONFLICT(game_item_id) DO UPDATE SET
               name=excluded.name, name_key=excluded.name_key,
               icon_id=COALESCE(excluded.icon_id, known_game_item.icon_id),
               pageid=COALESCE(excluded.pageid, known_game_item.pageid),
               source=excluded.source, updated=excluded.updated",
            params![id, base, key, icon, pageid, source],
        )?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// A catalog row the picker can add with a real game id.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogItem {
    pub game_item_id: i64,
    pub name: String,
    pub icon_id: Option<i64>,
    pub pageid: Option<i64>,
}

/// Search the known-game-item catalog by name substring (case-insensitive), name order.
pub fn catalog_search(query: &str, limit: usize) -> rusqlite::Result<Vec<CatalogItem>> {
    let c = crate::builds::conn()?;
    let like = format!("%{}%", norm(query));
    let mut stmt = c.prepare(
        "SELECT game_item_id,name,icon_id,pageid FROM known_game_item
           WHERE name_key LIKE ?1 ORDER BY name LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![like, limit as i64], |r| {
            Ok(CatalogItem {
                game_item_id: r.get(0)?,
                name: r.get(1)?,
                icon_id: r.get(2)?,
                pageid: r.get(3)?,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

/// How many items the catalog knows real game ids for (shown in the UI).
pub fn catalog_count() -> rusqlite::Result<i64> {
    let c = crate::builds::conn()?;
    c.query_row("SELECT COUNT(*) FROM known_game_item", [], |r| r.get(0))
}

/// A wiki item offered on the "all items" side of the picker. `game_item_id` is Some only
/// when the catalog already knows a real id for this name — otherwise adding it to a filter
/// needs an in-game encounter first (the honest "name-match" caveat).
#[derive(Debug, Clone, Serialize)]
pub struct WikiPick {
    pub pageid: i64,
    pub name: String,
    pub icon_id: Option<i64>,
    pub slot: Option<String>,
    pub game_item_id: Option<i64>,
}

/// Search all wiki items by name substring, attaching a real game id when the catalog has one.
pub fn wiki_search(query: &str, limit: usize, items: &BTreeMap<i64, Item>) -> Vec<WikiPick> {
    let q = norm(query);
    if q.is_empty() {
        return Vec::new();
    }
    // catalog name_key -> game id, to tag matches without a query per row
    let known: BTreeMap<String, i64> = crate::builds::conn()
        .ok()
        .and_then(|c| {
            let mut stmt = c.prepare("SELECT name_key,game_item_id FROM known_game_item").ok()?;
            let rows: BTreeMap<String, i64> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .ok()?
                .filter_map(Result::ok)
                .collect();
            Some(rows)
        })
        .unwrap_or_default();

    let mut out: Vec<WikiPick> = items
        .values()
        .filter(|it| norm(&it.name).contains(&q))
        .map(|it| WikiPick {
            pageid: it.pageid,
            name: it.name.clone(),
            icon_id: it.icon_id,
            slot: it.slot.clone(),
            game_item_id: known.get(&norm(&it.name)).copied(),
        })
        .collect();
    // exact-ish first (shorter names rank higher), then alpha; then cap
    out.sort_by(|a, b| a.name.len().cmp(&b.name.len()).then(a.name.cmp(&b.name)));
    out.truncate(limit);
    out
}

/// Harvest an inventory dump's game ids into the catalog. Returns how many rows were written.
pub fn import_inventory_catalog(path: &Path, items: &BTreeMap<i64, Item>) -> std::io::Result<usize> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let rows: Vec<(i64, String, Option<i64>)> = crate::inventory::harvest_game_items(&text)
        .into_iter()
        .map(|(id, name)| (id, name, None))
        .collect();
    catalog_upsert(&rows, "inventory", items).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_line_format() {
        let text = "#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME\n\
                    177815^3^581^Keg Mallet +4\n\
                    148590^2^10266^Mote of Infinitesimal Potential\n\
                    \n\
                    5040^4^768^Rusty Mining Pick +2\n";
        let e = parse_lf(text);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].item_id, 177815);
        assert_eq!(e[0].filter_id, 3);
        assert_eq!(e[0].icon_id, 581);
        assert_eq!(e[0].base_name, "Keg Mallet");
        assert_eq!(e[0].tier, 4);
        // mote has no "+N": base name is the whole thing, tier 0
        assert_eq!(e[1].base_name, "Mote of Infinitesimal Potential");
        assert_eq!(e[1].tier, 0);
    }

    #[test]
    fn round_trips_through_serialize() {
        let text = "#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME\n\
                    177815^3^581^Keg Mallet +4\n\
                    5040^4^768^Rusty Mining Pick +2\n";
        let out = serialize_lf(&parse_lf(text));
        assert_eq!(out, text);
    }

    #[test]
    fn skips_header_and_garbage_lines() {
        let text = "#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME\n\
                    not^enough\n\
                    abc^2^3^Bad Id\n\
                    7009^4^817^Rusty Spear +4\n";
        let e = parse_lf(text);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].item_id, 7009);
    }

    #[test]
    fn parses_lf_filename() {
        assert_eq!(parse_lf_filename("LF_Testchar_qeynos.ini"), Some(("Testchar".into(), "qeynos".into())));
        assert_eq!(parse_lf_filename("LF_Otherchar_neriak.ini"), Some(("Otherchar".into(), "neriak".into())));
        assert_eq!(parse_lf_filename("notafilter.ini"), None);
    }

    #[test]
    fn sanitizes_filename_tokens() {
        assert_eq!(sanitize_token(" Sars a "), "Sarsa");
        assert_eq!(sanitize_token("qeynos"), "qeynos");
    }

    // write to a scratch dir (no game folder / DB needed), then read the bytes back and
    // confirm the exact file the game expects: header, name, and a preserved backup.
    #[test]
    fn write_produces_game_format_and_backs_up() {
        let dir = std::env::temp_dir().join("eqlbuilder_lf_write_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let e1 = vec![LfEntry {
            item_id: 177815, filter_id: 3, icon_id: 581, name: "Keg Mallet +4".into(),
            base_name: "Keg Mallet".into(), tier: 4, pageid: None,
        }];
        let p = write_file("Testchar", "qeynos", &e1, Some(&dir)).unwrap();
        assert!(p.ends_with("LF_Testchar_qeynos.ini"));
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text, "#ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME\n177815^3^581^Keg Mallet +4\n");

        // a second write backs the first up and replaces it
        let e2 = vec![LfEntry {
            item_id: 7009, filter_id: 4, icon_id: 817, name: "Rusty Spear +4".into(),
            base_name: "Rusty Spear".into(), tier: 4, pageid: None,
        }];
        write_file("Testchar", "qeynos", &e2, Some(&dir)).unwrap();
        let bak = std::fs::read_to_string(dir.join("LF_Testchar_qeynos.ini.bak")).unwrap();
        assert!(bak.contains("Keg Mallet"), "backup keeps the prior contents");
        let now = std::fs::read_to_string(&p).unwrap();
        assert!(now.contains("Rusty Spear") && !now.contains("Keg Mallet"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_blank_character_or_city() {
        let dir = std::env::temp_dir();
        assert!(write_file("", "qeynos", &[], Some(&dir)).is_err());
        assert!(write_file("Foo", "  ", &[], Some(&dir)).is_err());
    }
}
