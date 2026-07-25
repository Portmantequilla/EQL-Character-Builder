//! Import a character's worn equipment from the game's `/outputfile inventory` dump.
//!
//! The dump is a tab-separated table with header `Location\tName\tID\tCount\tSlots`.
//! Two facts make it importable (both confirmed against a live inventory dump):
//!   * a worn item carries its UPGRADE TIER in the NAME as a " +N" suffix
//!     ("Wicked Sallet +6"), so we can recover tiers the game never stores elsewhere;
//!   * the GAME item id (col 3) is NOT the wiki pageid, so we bridge by NAME (minus the
//!     "+N"), exactly like spellbook.rs bridges spell ids through spells_us.txt.
//!
//! Each worn item's Exaltation augments appear as sub-rows `"<Slot>-SlotN"` naming the
//! socketed item. The build model has no augment field yet (the Exaltation feature is on
//! the roadmap), so we parse and REPORT them for display but do not silently drop them.
//!
//! Items absent from the wiki mirror (e.g. undocumented promo rewards like "Fippy's Paw")
//! come back in `unmatched` rather than vanishing — the importer never lies about coverage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eql_data::{canonical_slot, Item};
use serde::Serialize;

/// Fold backticks->apostrophes, collapse whitespace, lowercase — the same name key the
/// rest of the app matches on (db.rs / farm_sources use the identical fold).
pub(crate) fn norm(s: &str) -> String {
    s.replace('`', "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Strip a trailing " +N" upgrade suffix. "Onyx Earring +8" -> ("Onyx Earring", 8);
/// a name with no suffix (or a non-numeric one) returns tier 0 and the name unchanged.
pub(crate) fn split_tier(name: &str) -> (String, u32) {
    if let Some(idx) = name.rfind(" +") {
        let digits = &name[idx + 2..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(t) = digits.parse::<u32>() {
                return (name[..idx].trim().to_string(), t);
            }
        }
    }
    (name.trim().to_string(), 0)
}

/// The game's inventory slot label -> the paperdoll slot key(s) it fills. Doubled
/// physical slots (two ears/wrists/rings, two "Any" slots) list both keys and are
/// consumed left-to-right in the order the dump lists them. An unrecognized label
/// (bags, Bank, KeyRing, …) returns an empty slice and is ignored.
fn slot_targets(label: &str) -> &'static [&'static str] {
    match label.trim().to_ascii_lowercase().as_str() {
        "ear" | "ears" => &["EAR1", "EAR2"],
        "head" => &["HEAD"],
        "face" => &["FACE"],
        "neck" => &["NECK"],
        "shoulders" | "shoulder" => &["SHOULDERS"],
        "arms" | "arm" => &["ARMS"],
        "back" => &["BACK"],
        "wrist" | "wrists" => &["WRIST1", "WRIST2"],
        "range" | "ranged" => &["RANGE"],
        "hands" | "hand" => &["HANDS"],
        "primary" => &["PRIMARY"],
        "secondary" => &["SECONDARY"],
        "fingers" | "finger" | "ring" | "rings" => &["FINGER1", "FINGER2"],
        "chest" => &["CHEST"],
        "legs" | "leg" => &["LEGS"],
        "feet" | "foot" => &["FEET"],
        "waist" => &["WAIST"],
        "ammo" => &["AMMO"],
        "any slot" | "any" => &["ANY1", "ANY2"],
        _ => &[],
    }
}

/// A worn slot we resolved to a wiki item.
#[derive(Debug, Clone, Serialize)]
pub struct MatchedSlot {
    pub slot: String,      // paperdoll key (EAR1, PRIMARY, …)
    pub pageid: i64,       // resolved wiki pageid
    pub base_name: String, // name minus the "+N"
    pub tier: u32,         // 0..10, read from the "+N" suffix
    pub game_name: String, // the raw name from the dump, incl. "+N"
}

/// A worn item the wiki mirror doesn't have (kept, never dropped).
#[derive(Debug, Clone, Serialize)]
pub struct UnmatchedSlot {
    pub slot: String, // paperdoll key it would have filled ("" if none was free)
    pub game_name: String,
    pub base_name: String,
    pub tier: u32,
    pub reason: String, // "not in wiki data" | "no free <slot> slot"
}

/// One Exaltation augment socketed into a worn item.
#[derive(Debug, Clone, Serialize)]
pub struct Exaltation {
    pub slot: String,   // parent paperdoll key
    pub socket: String, // socket index as written ("7", "8", …)
    pub name: String,   // e.g. "Wicked Sallet (Exaltation)"
    /// socket TYPE the index maps to (FOCUS/CLICK/WORN/PROC/ORNAMENTATION) — verified
    /// 7/7 against a live character's sockets vs the DB's effect kinds (2026-07-15)
    pub socket_type: Option<String>,
    /// the augment's SOURCE item resolved by name (minus " (Exaltation)"), when known
    pub source_pageid: Option<i64>,
}

/// Dump socket index -> augment socket type. Order matches the in-game item window
/// (Ornamentation, Focus/Click/Worn/Proc Exaltation); 7=FOCUS/8=CLICK/10=PROC confirmed
/// by cross-checking a live dump's sources against their DB effect kinds (7/7 match);
/// 1=ORNAMENTATION and 9=WORN follow from the window order.
fn socket_type(index: &str) -> Option<&'static str> {
    match index {
        "1" => Some("ORNAMENTATION"),
        "7" => Some("FOCUS"),
        "8" => Some("CLICK"),
        "9" => Some("WORN"),
        "10" => Some("PROC"),
        _ => None,
    }
}

/// The result of importing an inventory dump.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InventoryImport {
    /// character name parsed from the filename (`<Name>_<city>-Inventory.txt`)
    pub character: Option<String>,
    /// paperdoll slot -> resolved wiki pageid (matched worn items only)
    pub equipment: BTreeMap<String, i64>,
    /// paperdoll slot -> upgrade tier (matched worn items with tier > 0)
    pub equipment_tiers: BTreeMap<String, u32>,
    /// paperdoll slot -> socket type -> source pageid: Exaltations whose socket index
    /// mapped and whose source item resolved — ready to merge into build.augments
    pub augments: BTreeMap<String, BTreeMap<String, i64>>,
    pub matched: Vec<MatchedSlot>,
    pub unmatched: Vec<UnmatchedSlot>,
    pub exaltations: Vec<Exaltation>,
    /// absolute path we parsed (echoed back so the UI can show/remember it)
    pub source_file: String,
}

/// Best pageid for a game item name going into `canon` slot: prefer an item whose wear
/// slots actually include that slot (disambiguates same-named items); else the first.
fn resolve_name(by_name: &BTreeMap<String, Vec<i64>>, items: &BTreeMap<i64, Item>,
                name_key: &str, canon: &str) -> Option<i64> {
    let ids = by_name.get(name_key)?;
    if canon != "ANY" {
        if let Some(id) = ids.iter().copied().find(|id| {
            items.get(id).is_some_and(|it| {
                it.slots.iter().any(|s| s.eq_ignore_ascii_case(canon))
                    || it.slot.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(canon))
            })
        }) {
            return Some(id);
        }
    }
    ids.first().copied()
}

/// Parse an inventory dump's text into a resolved import. `items` is the wiki snapshot
/// (pageid -> Item); `filename` is used only to recover the character name.
pub fn parse_inventory(text: &str, items: &BTreeMap<i64, Item>, filename: &str) -> InventoryImport {
    // name -> [pageid…] (a name can repeat across items; keep them all, lowest id first)
    let mut by_name: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for it in items.values() {
        by_name.entry(norm(&it.name)).or_default().push(it.pageid);
    }
    for v in by_name.values_mut() {
        v.sort_unstable();
    }

    let mut out = InventoryImport {
        character: character_from_filename(filename),
        source_file: filename.to_string(),
        ..Default::default()
    };
    // how many of each doubled label we've assigned so far (Ear #0 -> EAR1, #1 -> EAR2)
    let mut used: BTreeMap<&'static str, usize> = BTreeMap::new();

    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let location = cols[0].trim();
        let raw_name = cols[1].trim();
        if location.eq_ignore_ascii_case("Location") {
            continue; // header
        }

        // Exaltation / augment sub-row: "<Slot>-Slot7". The game emits these right after
        // their parent worn row, so the most-recently-assigned key for this label IS the
        // parent — resolve to the specific paperdoll key (EAR1 vs EAR2) so the two members
        // of a doubled slot don't collapse to the same "Ear". Record (parent worn, socket
        // filled) and move on — augments aren't worn items themselves.
        if let Some((prefix, socket)) = location.rsplit_once("-Slot") {
            let ptargets = slot_targets(prefix);
            if !ptargets.is_empty()
                && !raw_name.is_empty()
                && !raw_name.eq_ignore_ascii_case("Empty")
            {
                let assigned = used.get(ptargets[0]).copied().unwrap_or(0);
                let parent = assigned
                    .checked_sub(1)
                    .and_then(|i| ptargets.get(i).copied())
                    .unwrap_or(ptargets[0]); // no worn row seen yet: default to the first key
                let socket = socket.trim().to_string();
                let stype = socket_type(&socket);
                // "<Source> (Exaltation)" -> the source item, matched by name
                let source_pageid = raw_name
                    .strip_suffix(" (Exaltation)")
                    .and_then(|base| by_name.get(&norm(base)))
                    .and_then(|ids| ids.first().copied());
                if let (Some(st), Some(pid)) = (stype, source_pageid) {
                    out.augments
                        .entry(parent.to_string())
                        .or_default()
                        .insert(st.to_string(), pid);
                }
                out.exaltations.push(Exaltation {
                    slot: parent.to_string(),
                    socket,
                    name: raw_name.to_string(),
                    socket_type: stype.map(|s| s.to_string()),
                    source_pageid,
                });
            }
            continue;
        }

        let targets = slot_targets(location);
        if targets.is_empty() {
            continue; // bags, Bank, SharedBank, KeyRing, General… — not worn
        }
        // consume the next target key for this label, even for Empty, so slot numbering
        // stays aligned with the game's fixed worn-slot order
        let n = used.entry(targets[0]).or_insert(0);
        let slot_key = targets.get(*n).copied();
        *n += 1;
        if raw_name.is_empty() || raw_name.eq_ignore_ascii_case("Empty") {
            continue; // empty worn slot: nothing to import, but the index was consumed
        }

        let (base, tier) = split_tier(raw_name);
        let Some(slot_key) = slot_key else {
            // more filled items of this label than we have keys (e.g. a 3rd ring)
            out.unmatched.push(UnmatchedSlot {
                slot: String::new(),
                game_name: raw_name.to_string(),
                base_name: base.clone(),
                tier,
                reason: format!("no free {location} slot"),
            });
            continue;
        };
        let canon = canonical_slot(slot_key);
        match resolve_name(&by_name, items, &norm(&base), canon) {
            Some(pid) => {
                out.equipment.insert(slot_key.to_string(), pid);
                if tier > 0 {
                    out.equipment_tiers.insert(slot_key.to_string(), tier);
                }
                out.matched.push(MatchedSlot {
                    slot: slot_key.to_string(),
                    pageid: pid,
                    base_name: base,
                    tier,
                    game_name: raw_name.to_string(),
                });
            }
            None => out.unmatched.push(UnmatchedSlot {
                slot: slot_key.to_string(),
                game_name: raw_name.to_string(),
                base_name: base,
                tier,
                reason: "not in wiki data".to_string(),
            }),
        }
    }
    out
}

/// Every distinct (game_item_id, base_name) an inventory dump mentions — worn, bagged,
/// banked, all of it. This is the loot-filter catalog's richest seed: unlike the worn-slot
/// import above it keeps EVERYTHING the character carries, because a loot filter cares about
/// any item that can drop, not just what's equipped. Returns (id, base_name) with the "+N"
/// tier stripped and duplicate ids collapsed (first name wins). Rows with id 0 / "Empty" /
/// non-numeric ids are skipped.
pub fn harvest_game_items(text: &str) -> Vec<(i64, String)> {
    let mut seen: BTreeMap<i64, String> = BTreeMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let raw_name = cols[1].trim();
        if raw_name.is_empty()
            || raw_name.eq_ignore_ascii_case("Empty")
            || raw_name.eq_ignore_ascii_case("Name")
        {
            continue; // empty slot or the header row
        }
        let Ok(id) = cols[2].trim().parse::<i64>() else {
            continue;
        };
        if id <= 0 {
            continue;
        }
        // strip the " (Exaltation)" socket tag and the "+N" tier so the catalog keys on the
        // base item exactly the way the filter file's game id does (tier-independent)
        let clean = raw_name.strip_suffix(" (Exaltation)").unwrap_or(raw_name);
        let (base, _tier) = split_tier(clean);
        seen.entry(id).or_insert(base);
    }
    seen.into_iter().collect()
}

/// "Testchar_qeynos-Inventory.txt" -> Some("Testchar"). The game appends
/// "_<city>-Inventory.txt"; character names are single words, so the part before the
/// first underscore is the name. Falls back to the stem when there's no underscore.
fn character_from_filename(filename: &str) -> Option<String> {
    let base = Path::new(filename).file_name()?.to_string_lossy();
    let stem = base
        .strip_suffix(".txt")
        .unwrap_or(&base)
        .trim_end_matches("-Inventory")
        .trim_end_matches("-inventory");
    let name = stem.split('_').next().unwrap_or(stem).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Read + parse an inventory file.
pub fn import_file(path: &Path, items: &BTreeMap<i64, Item>) -> std::io::Result<InventoryImport> {
    // lossy: dumps are ASCII, but a stray non-UTF-8 byte must not abort the whole import
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let name = path.to_string_lossy();
    let mut imp = parse_inventory(&text, items, &name);
    imp.source_file = path.display().to_string();
    Ok(imp)
}

// ------------------------------------------------------------------ EQL directory
// `/outputfile inventory` writes into the game's own folder, whose location varies per
// user (custom drives). We remember the folder the user last imported from in app_meta,
// so after the first Browse the app can auto-find the newest dump without asking again.

const EQL_DIR_KEY: &str = "eql_game_dir";

fn get_meta(key: &str) -> Option<String> {
    let c = crate::builds::conn().ok()?;
    c.query_row(
        "SELECT value FROM app_meta WHERE key=?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .filter(|s| !s.is_empty())
}

fn set_meta(key: &str, value: &str) {
    if let Ok(c) = crate::builds::conn() {
        let _ = c.execute(
            "INSERT INTO app_meta(key,value) VALUES(?1,?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![key, value],
        );
    }
}

/// Remember an EQL game directory (the folder holding the *-Inventory.txt files).
pub fn remember_eql_dir(dir: &Path) {
    if dir.is_dir() {
        set_meta(EQL_DIR_KEY, &dir.display().to_string());
    }
}

/// Best guess at the EQL game folder: remembered value -> EQL_GAME_DIR env -> the common
/// default "E:/EQL". Returns None if none of those exist on disk.
pub fn guess_eql_dir() -> Option<PathBuf> {
    if let Some(d) = get_meta(EQL_DIR_KEY) {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(d) = std::env::var("EQL_GAME_DIR") {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
    }
    let default = PathBuf::from("E:/EQL");
    default.is_dir().then_some(default)
}

/// One `*-Inventory.txt` the app found, newest first.
#[derive(Debug, Clone, Serialize)]
pub struct InventoryFile {
    pub path: String,
    pub name: String,          // file name only
    pub character: Option<String>,
    pub modified_epoch: u64,   // seconds since epoch (0 if unknown) — for "newest" sort/label
}

/// What `list_inventory_files` reports: the folder it scanned and the dumps in it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InventoryScan {
    pub dir: Option<String>,
    pub files: Vec<InventoryFile>,
}

/// List `*-Inventory.txt` files in the (remembered/guessed) EQL folder, newest first.
pub fn list_inventory_files() -> InventoryScan {
    let Some(dir) = guess_eql_dir() else {
        return InventoryScan::default();
    };
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with("-inventory.txt") {
                continue;
            }
            let modified_epoch = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            files.push(InventoryFile {
                path: path.display().to_string(),
                character: character_from_filename(&name),
                name,
                modified_epoch,
            });
        }
    }
    files.sort_by(|a, b| b.modified_epoch.cmp(&a.modified_epoch).then(a.name.cmp(&b.name)));
    InventoryScan { dir: Some(dir.display().to_string()), files }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(pageid: i64, name: &str, slot: &str) -> Item {
        Item {
            pageid,
            name: name.to_string(),
            slot: Some(slot.to_string()),
            slots: vec![slot.to_string()],
            ..Default::default()
        }
    }

    fn snapshot() -> BTreeMap<i64, Item> {
        [
            item(1, "Wicked Sallet", "HEAD"),
            item(2, "Onyx Earring", "EAR"),
            item(3, "Diamondine Earring", "EAR"),
            item(4, "Enchanted Fine Steel Morning Star", "SECONDARY"),
            item(5, "Moonstone Ring", "FINGER"),
            item(6, "Platinum Ring", "FINGER"),
            item(7, "Silver Plated War Sword", "PRIMARY"),
        ]
        .into_iter()
        .map(|it| (it.pageid, it))
        .collect()
    }

    // a trimmed real-format dump: header, doubled slots, an Empty, a "+N", an Exaltation
    // sub-row, an undocumented item, and a non-worn bag row that must be ignored.
    const DUMP: &str = "Location\tName\tID\tCount\tSlots\n\
        Ear\tDiamondine Earring +4\t10165\t1\t0\n\
        Head\tWicked Sallet +6\t177814\t1\t0\n\
        Head-Slot7\tWicked Sallet (Exaltation)\t9001\t1\t0\n\
        Ear\tOnyx Earring +8\t10354\t1\t0\n\
        Back\tEmpty\t0\t0\t0\n\
        Range\tFippy's Paw +4\t60396\t1\t0\n\
        Secondary\tEnchanted Fine Steel Morning Star +10\t5214\t1\t0\n\
        Fingers\tMoonstone Ring +2\t10150\t1\t0\n\
        Fingers\tPlatinum Ring +6\t13734\t1\t0\n\
        Any Slot\tEmpty\t0\t0\t0\n\
        Any Slot\tSilver Plated War Sword +5\t1876\t1\t0\n\
        General1\tBag of Sewing\t999\t1\t10\n";

    #[test]
    fn splits_tier_from_name() {
        assert_eq!(split_tier("Onyx Earring +8"), ("Onyx Earring".into(), 8));
        assert_eq!(split_tier("Wicked Sallet +6"), ("Wicked Sallet".into(), 6));
        assert_eq!(split_tier("Pristine Studded Leather Gloves"), ("Pristine Studded Leather Gloves".into(), 0));
        // a "+" that isn't a tier suffix must not be misread
        assert_eq!(split_tier("Cloak of Flames +x"), ("Cloak of Flames +x".into(), 0));
    }

    #[test]
    fn character_parsed_from_filename() {
        assert_eq!(character_from_filename("Testchar_qeynos-Inventory.txt").as_deref(), Some("Testchar"));
        assert_eq!(character_from_filename(r"E:\EQL\Foo_freeport-Inventory.txt").as_deref(), Some("Foo"));
    }

    #[test]
    fn maps_doubled_slots_and_tiers() {
        let imp = parse_inventory(DUMP, &snapshot(), "Testchar_qeynos-Inventory.txt");
        assert_eq!(imp.character.as_deref(), Some("Testchar"));
        // two ears land in EAR1 then EAR2, in file order
        assert_eq!(imp.equipment.get("EAR1"), Some(&3)); // Diamondine
        assert_eq!(imp.equipment.get("EAR2"), Some(&2)); // Onyx
        assert_eq!(imp.equipment_tiers.get("EAR2"), Some(&8));
        // two rings -> FINGER1/FINGER2
        assert_eq!(imp.equipment.get("FINGER1"), Some(&5));
        assert_eq!(imp.equipment.get("FINGER2"), Some(&6));
        // head + tier
        assert_eq!(imp.equipment.get("HEAD"), Some(&1));
        assert_eq!(imp.equipment_tiers.get("HEAD"), Some(&6));
        // the second (filled) "Any Slot" is ANY2 because the first Empty consumed ANY1
        assert_eq!(imp.equipment.get("ANY2"), Some(&7));
        // secondary + max tier
        assert_eq!(imp.equipment_tiers.get("SECONDARY"), Some(&10));
    }

    #[test]
    fn unmatched_item_kept_not_dropped() {
        let imp = parse_inventory(DUMP, &snapshot(), "x-Inventory.txt");
        assert_eq!(imp.unmatched.len(), 1);
        assert_eq!(imp.unmatched[0].base_name, "Fippy's Paw");
        assert_eq!(imp.unmatched[0].tier, 4);
        assert_eq!(imp.unmatched[0].slot, "RANGE");
    }

    #[test]
    fn exaltation_subrow_recorded_not_equipped() {
        let imp = parse_inventory(DUMP, &snapshot(), "x-Inventory.txt");
        assert_eq!(imp.exaltations.len(), 1);
        // parent resolves to the specific paperdoll key, not the raw "Head" label
        assert_eq!(imp.exaltations[0].slot, "HEAD");
        assert_eq!(imp.exaltations[0].socket, "7");
        // socket 7 = Focus Exaltation; the source resolves by name (minus " (Exaltation)")
        assert_eq!(imp.exaltations[0].socket_type.as_deref(), Some("FOCUS"));
        assert_eq!(imp.exaltations[0].source_pageid, Some(1));
        assert_eq!(imp.augments.get("HEAD").and_then(|m| m.get("FOCUS")), Some(&1));
        // the Exaltation row must NOT have been counted as a worn HEAD item
        assert_eq!(imp.matched.iter().filter(|m| m.slot == "HEAD").count(), 1);
    }

    // regression: two augmented ears at the same socket index must NOT collapse to the
    // same parent slot (that produced a Svelte each_key_duplicate crash in the summary).
    #[test]
    fn doubled_slot_exaltations_disambiguate() {
        let dump = "Location\tName\tID\tCount\tSlots\n\
            Ear\tDiamondine Earring +4\t10165\t1\t10\n\
            Ear-Slot7\tDiamondine Earring (Exaltation)\t10165\t1\t10\n\
            Ear\tOnyx Earring +8\t10354\t1\t10\n\
            Ear-Slot7\tOnyx Earring (Exaltation)\t10354\t1\t10\n";
        let imp = parse_inventory(dump, &snapshot(), "x-Inventory.txt");
        assert_eq!(imp.exaltations.len(), 2);
        assert_eq!(imp.exaltations[0].slot, "EAR1"); // first ear's augment
        assert_eq!(imp.exaltations[1].slot, "EAR2"); // second ear's augment
        // (slot, socket) is now a unique key across both — no collision
        let keys: std::collections::BTreeSet<_> =
            imp.exaltations.iter().map(|x| (&x.slot, &x.socket)).collect();
        assert_eq!(keys.len(), 2);
        // each ear's augment lands in ITS OWN slot's socket map
        assert_eq!(imp.augments.get("EAR1").and_then(|m| m.get("FOCUS")), Some(&3));
        assert_eq!(imp.augments.get("EAR2").and_then(|m| m.get("FOCUS")), Some(&2));
    }

    #[test]
    fn harvest_catches_all_rows_dedup_and_strips_suffixes() {
        // worn, bagged, an Exaltation sub-row, an Empty, and a duplicate id must all resolve
        // to distinct base names keyed by game id (the loot-filter catalog seed)
        let dump = "Location\tName\tID\tCount\tSlots\n\
            Head\tWicked Sallet +6\t177814\t1\t10\n\
            Head-Slot7\tWicked Sallet (Exaltation)\t177814\t1\t10\n\
            General1\tKeg Mallet +7\t177815\t1\t10\n\
            Bank1\tRusty Spear\t7009\t1\t0\n\
            Bank2\tEmpty\t0\t0\t0\n";
        let mut got = harvest_game_items(dump);
        got.sort();
        // duplicate id 177814 collapses to one row; the "+N" and " (Exaltation)" are stripped
        assert_eq!(got, vec![
            (7009, "Rusty Spear".to_string()),
            (177814, "Wicked Sallet".to_string()),
            (177815, "Keg Mallet".to_string()),
        ]);
    }

    #[test]
    fn non_worn_rows_ignored() {
        let imp = parse_inventory(DUMP, &snapshot(), "x-Inventory.txt");
        // "General1 / Bag of Sewing" is a bag, never a worn slot
        assert!(imp.matched.iter().all(|m| m.base_name != "Bag of Sewing"));
        assert!(imp.equipment.values().all(|&p| p != 999));
    }
}
