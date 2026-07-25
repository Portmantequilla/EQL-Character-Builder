//! SQLite access for the Tauri layer. The engine never sees these types (plan §4.1).
//! Loads the disposable wiki mirror into an in-memory Snapshot (cached per process).
use eql_data::{BuffLine, BuffLineMember, Item, PetSummonInfo};
use eql_engine::{Snapshot, SpellClassLevels, SpellNames};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

/// wiki.db location. Plan §2: DBs live in %LOCALAPPDATA%/EQLBuilder, NEVER OneDrive.
/// Resolution order: EQL_WIKI_DB env (tests/tooling) -> %LOCALAPPDATA% copy ->
/// the repo's db/eql.db found by walking up from the CWD (dev fallback).
pub fn wiki_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("EQL_WIKI_DB") {
        return PathBuf::from(p);
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("EQLBuilder").join("wiki.db");
        if p.exists() { return p; }
    }
    for up in ["../db/eql.db", "../../db/eql.db", "../../../db/eql.db"] {
        let p = PathBuf::from(up);
        if p.exists() { return p; }
    }
    PathBuf::from("../db/eql.db")
}

fn conn() -> rusqlite::Result<Connection> {
    // READ_ONLY: the wiki mirror is never written by the app, and a bad path must
    // error loudly instead of silently creating an empty database (verify finding)
    Connection::open_with_flags(
        wiki_db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Missing table (data importer not yet run) degrades to empty, never a hard error.
fn table_exists(c: &Connection, name: &str) -> bool {
    c.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

pub fn load_items_full(c: &Connection) -> rusqlite::Result<BTreeMap<i64, Item>> {
    let has_req = c
        .prepare("SELECT required_level FROM items LIMIT 1")
        .map(|_| true)
        .unwrap_or(false);
    let req_cols = if has_req {
        "i.required_level, i.recommended_level"
    } else {
        "NULL, NULL"
    };
    // weight/size are backfilled by scripts/extract_item_dims.py — degrade on old DBs
    let has_dims = c
        .prepare("SELECT weight FROM items LIMIT 1")
        .map(|_| true)
        .unwrap_or(false);
    let dim_cols = if has_dims { "i.weight, i.size" } else { "NULL, NULL" };
    // canonical is added by scripts/import_supplemental.py — degrade to "all real" on old DBs
    let has_canon = c
        .prepare("SELECT canonical FROM items LIMIT 1")
        .map(|_| true)
        .unwrap_or(false);
    let canon_col = if has_canon { "i.canonical" } else { "1" };
    // is_epic is added by scripts/mark_epic_items.py — degrade to "nothing epic" on old DBs
    let has_epic = c
        .prepare("SELECT is_epic FROM items LIMIT 1")
        .map(|_| true)
        .unwrap_or(false);
    let epic_col = if has_epic { "i.is_epic" } else { "0" };
    let mut items: BTreeMap<i64, Item> = BTreeMap::new();
    let sql = format!(
        "SELECT i.pageid, i.name, i.slot, i.ac, i.dmg, i.atk_delay, i.weapon_skill, \
                i.haste_pct, i.worn_effect, i.focus_effect, i.click_effect, i.era, {req_cols}, \
                i.icon_id, i.flags, {dim_cols}, i.merchant_value, {canon_col}, {epic_col} \
         FROM items i"
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(Item {
            pageid: r.get(0)?, name: r.get(1)?, slot: r.get(2)?, icon_id: r.get(14)?,
            slots: Vec::new(), classes: Vec::new(), races: Vec::new(), deities: Vec::new(),
            ac: r.get(3)?, dmg: r.get(4)?, atk_delay: r.get(5)?, weapon_skill: r.get(6)?,
            haste_pct: r.get(7)?, required_level: r.get(12)?, recommended_level: r.get(13)?,
            stats: BTreeMap::new(),
            worn_effect: r.get(8)?, focus_effect: r.get(9)?, click_effect: r.get(10)?,
            era: r.get(11)?,
            flags: r.get::<_, Option<String>>(15)?.filter(|s| !s.is_empty()),
            weight: r.get(16)?, size: r.get(17)?,
            merchant_value: r.get::<_, Option<String>>(18)?.filter(|s| !s.is_empty()),
            non_canonical: r.get::<_, i64>(19).unwrap_or(1) == 0,
            is_epic: r.get::<_, i64>(20).unwrap_or(0) != 0,
        })
    })?;
    for it in rows.flatten() {
        items.insert(it.pageid, it);
    }
    // bulk-attach children (one pass each, no per-item queries)
    let mut cstmt = c.prepare("SELECT pageid, class FROM item_classes")?;
    for row in cstmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (pid, cl) = row?;
        if let Some(it) = items.get_mut(&pid) { it.classes.push(cl); }
    }
    let mut sstmt = c.prepare("SELECT pageid, stat, value FROM item_stats")?;
    for row in sstmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (pid, k, v) = row?;
        if let Some(it) = items.get_mut(&pid) { it.stats.insert(k, v); }
    }
    if table_exists(c, "item_slots") {
        let mut slstmt = c.prepare("SELECT pageid, slot FROM item_slots")?;
        for row in slstmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (pid, s) = row?;
            if let Some(it) = items.get_mut(&pid) { it.slots.push(s); }
        }
    }
    let mut rstmt = c.prepare("SELECT pageid, race FROM item_races")?;
    for row in rstmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (pid, s) = row?;
        if let Some(it) = items.get_mut(&pid) {
            it.races.push(s.trim_end_matches("<BR>").to_string());
        }
    }
    if table_exists(c, "item_deity") {
        let mut dstmt = c.prepare("SELECT pageid, deity FROM item_deity")?;
        for row in dstmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (pid, s) = row?;
            if let Some(it) = items.get_mut(&pid) { it.deities.push(s); }
        }
    }
    Ok(items)
}

pub fn load_buff_lines(c: &Connection) -> rusqlite::Result<Vec<BuffLine>> {
    if !table_exists(c, "buff_line") {
        return Ok(Vec::new());
    }
    let mut lstmt = c.prepare(
        "SELECT id, name, category, statistic, effect_slot, bard_layer \
         FROM buff_line ORDER BY id",
    )?;
    let mut mstmt = c.prepare(
        "SELECT spell_id, member_name_raw, source_kind, value_base, value_max_instrument, \
                source_items, is_group, is_self_only \
         FROM buff_line_member WHERE buff_line_id=?1 ORDER BY id",
    )?;
    let lines = lstmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, BuffLine {
            name: r.get(1)?, category: r.get(2)?, statistic: r.get(3)?,
            effect_slot: r.get(4)?, bard_layer: r.get(5)?, members: Vec::new(),
        }))
    })?;
    let mut out = Vec::new();
    for row in lines {
        let (lid, mut line) = row?;
        let members = mstmt.query_map([lid], |r| {
            Ok(BuffLineMember {
                spell_id: r.get(0)?, member_name_raw: r.get(1)?, source_kind: r.get(2)?,
                value_base: r.get(3)?, value_max_instrument: r.get(4)?,
                source_items: r.get(5)?,
                is_group: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                is_self_only: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
            })
        })?;
        line.members = members.filter_map(|m| m.ok()).collect();
        out.push(line);
    }
    Ok(out)
}

pub fn load_spell_class_levels(c: &Connection) -> rusqlite::Result<SpellClassLevels> {
    let mut stmt = c.prepare(
        "SELECT l.spell_id, cl.abbr, l.required_class_level \
         FROM spell_class_level l JOIN class cl ON cl.id = l.class_id",
    )?;
    let mut out = SpellClassLevels::new();
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, u32>(2)?))
    })? {
        let (sid, abbr, lvl) = row?;
        let e = out.entry(sid).or_default().entry(abbr).or_insert(lvl);
        if lvl < *e { *e = lvl; }
    }
    Ok(out)
}

pub fn load_spell_names(c: &Connection) -> rusqlite::Result<SpellNames> {
    let mut stmt = c.prepare("SELECT id, COALESCE(page_title, name) FROM spell")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

fn load_race_stats(c: &Connection) -> BTreeMap<String, BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    if !table_exists(c, "race_base_stats") {
        return out;
    }
    if let Ok(mut stmt) = c.prepare(
        "SELECT r.name, s.stat, s.value FROM race_base_stats s JOIN race r ON r.id = s.race_id",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        }) {
            for row in rows.flatten() {
                out.entry(row.0).or_insert_with(BTreeMap::new).insert(row.1, row.2);
            }
        }
    }
    out
}

fn load_class_mods(c: &Connection) -> BTreeMap<String, BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    if !table_exists(c, "class_stat_mod") {
        return out;
    }
    if let Ok(mut stmt) = c.prepare(
        "SELECT cl.abbr, m.str, m.sta, m.agi, m.dex, m.wis, m.intel, m.cha \
         FROM class_stat_mod m JOIN class cl ON cl.id = m.class_id",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            let abbr: String = r.get(0)?;
            let vals: [i64; 7] = [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                                  r.get(5)?, r.get(6)?, r.get(7)?];
            Ok((abbr, vals))
        }) {
            for (abbr, vals) in rows.flatten() {
                let m: BTreeMap<String, i64> = ["STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA"]
                    .iter()
                    .zip(vals)
                    .map(|(k, v)| (k.to_string(), v))
                    .collect();
                out.insert(abbr, m);
            }
        }
    }
    out
}

fn load_pet_summons(c: &Connection, names: &SpellNames, clevels: &SpellClassLevels)
    -> BTreeMap<i64, PetSummonInfo>
{
    let mut out = BTreeMap::new();
    let Ok(mut stmt) = c.prepare(
        "SELECT spell_id, pet_classes, base_pet_level, pet_hp_numeric, pet_max_hit \
         FROM spell_pet_summon",
    ) else { return out };
    if let Ok(rows) = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?, r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?))
    }) {
        for (sid, pc, lvl, hp, hit) in rows.flatten() {
            out.insert(sid, PetSummonInfo {
                spell_id: sid,
                name: names.get(&sid).cloned().unwrap_or_else(|| format!("spell#{sid}")),
                base_pet_level: lvl,
                pet_classes: pc,
                pet_hp: hp,
                pet_max_hit: hit,
                class_levels: clevels.get(&sid).cloned().unwrap_or_default(),
                estimate_confidence: None,
            });
        }
    }
    // user research workbook (scripts/import_pet_estimates.py): fill the NULLs.
    // Wiki-tested values always win — estimates only cover what the wiki left blank,
    // and any summon that took an estimate carries the sheet's confidence label.
    if table_exists(c, "pet_summon_estimate") {
        if let Ok(mut estmt) = c.prepare(
            "SELECT spell_id, base_pet_level, hp_est, max_hit_est, pet_classes, confidence \
             FROM pet_summon_estimate",
        ) {
            if let Ok(rows) = estmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?, r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?))
            }) {
                for (sid, lvl, hp, hit, pc, conf) in rows.flatten() {
                    let Some(info) = out.get_mut(&sid) else { continue };
                    let mut used_estimate = false;
                    if info.base_pet_level.is_none() && lvl.is_some() {
                        info.base_pet_level = lvl;
                        used_estimate = true;
                    }
                    if info.pet_hp.is_none() && hp.is_some() {
                        info.pet_hp = hp;
                        used_estimate = true;
                    }
                    if info.pet_max_hit.is_none() && hit.is_some() {
                        info.pet_max_hit = hit;
                        used_estimate = true;
                    }
                    if info.pet_classes.is_none() && pc.is_some() {
                        info.pet_classes = pc;
                    }
                    if used_estimate {
                        info.estimate_confidence =
                            conf.or_else(|| Some("estimate".to_string()));
                    }
                }
            }
        }
    }
    out
}

fn load_combinations(c: &Connection) -> BTreeMap<i64, Vec<String>> {
    let mut out: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    if !table_exists(c, "spell_buff_line") {
        return out;
    }
    if let Ok(mut stmt) = c.prepare(
        "SELECT sbl.spell_id, bl.name FROM spell_buff_line sbl \
         JOIN buff_line bl ON bl.id = sbl.buff_line_id ORDER BY sbl.spell_id, bl.id",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        }) {
            for (sid, line) in rows.flatten() {
                out.entry(sid).or_default().push(line);
            }
        }
    }
    // only true combinations (>1 line) matter to the resolver
    out.retain(|_, v| v.len() > 1);
    out
}

fn load_item_effects(c: &Connection) -> BTreeMap<i64, Vec<eql_data::ItemEffect>> {
    let mut out: BTreeMap<i64, Vec<eql_data::ItemEffect>> = BTreeMap::new();
    if !table_exists(c, "item_effect") {
        return out;
    }
    if let Ok(mut stmt) = c.prepare(
        "SELECT pageid, effect_name, activation_type, required_level, spell_id \
         FROM item_effect ORDER BY id",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, eql_data::ItemEffect {
                effect_name: r.get(1)?, activation_type: r.get(2)?,
                required_level: r.get(3)?, spell_id: r.get(4)?,
            }))
        }) {
            for (pid, e) in rows.flatten() {
                out.entry(pid).or_default().push(e);
            }
        }
    }
    out
}

fn load_aas(c: &Connection) -> Vec<eql_data::AaAbility> {
    if !table_exists(c, "aa") {
        return Vec::new();
    }
    let Ok(mut stmt) = c.prepare(
        "SELECT id, name, category, class_abbr, max_rank, cost_json, cost_complete, \
                required_level, description FROM aa ORDER BY category, class_abbr, name",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |r| {
        let cost_json: String = r.get(5)?;
        Ok(eql_data::AaAbility {
            id: r.get(0)?, name: r.get(1)?, category: r.get(2)?, class_abbr: r.get(3)?,
            max_rank: r.get(4)?,
            costs: serde_json::from_str(&cost_json).unwrap_or_default(),
            cost_complete: r.get::<_, i64>(6)? != 0,
            required_level: r.get(7)?, description: r.get(8)?,
        })
    });
    match rows {
        Ok(rs) => rs.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn load_pet_slot_rule() -> eql_engine::PetSlotRule {
    // builds.db pet_equipment_rule (highest id wins); defaults are the same seed
    let Ok(c) = crate::builds::conn() else { return Default::default() };
    let json: Option<String> = c
        .query_row("SELECT rule_json FROM pet_equipment_rule ORDER BY id DESC LIMIT 1",
                   [], |r| r.get(0))
        .ok();
    let Some(json) = json else { return Default::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else { return Default::default() };
    let mut rule = eql_engine::PetSlotRule::default();
    if let Some(b) = v.get("slots_base").and_then(|x| x.as_u64()) {
        rule.slots_base = b as usize;
    }
    if let Some(m) = v.get("bonus").and_then(|x| x.as_object()) {
        rule.bonus = m.iter()
            .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n as usize)))
            .collect();
    }
    rule
}

fn load_spell_durations(c: &Connection) -> BTreeMap<i64, String> {
    let mut out = BTreeMap::new();
    if let Ok(mut stmt) = c.prepare(
        "SELECT id, duration_raw FROM spell WHERE duration_raw IS NOT NULL AND duration_raw != ''",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        }) {
            for (sid, d) in rows.flatten() {
                out.insert(sid, d);
            }
        }
    }
    out
}

/// Flat stat lines per spell for the worn-effect stat fold: spell_effect rows with a
/// stat and a flat amount. Percent rows are EXCLUDED (worn haste already counts via
/// items.haste_pct — including it here would double-count). "per tick" HP/MANA lines
/// map to the REGEN stats.
fn load_spell_stat_effects(c: &Connection) -> BTreeMap<i64, Vec<(String, f64)>> {
    let mut out: BTreeMap<i64, Vec<(String, f64)>> = BTreeMap::new();
    if !table_exists(c, "spell_effect") {
        return out;
    }
    let Ok(mut stmt) = c.prepare(
        "SELECT spell_id, opcode, base_amount, COALESCE(raw_text,'') FROM spell_effect \
         WHERE stat IS NOT NULL AND base_amount IS NOT NULL AND COALESCE(is_percent,0)=0",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?,
            r.get::<_, String>(3)?))
    }) else {
        return out;
    };
    for (sid, opcode, amount, raw) in rows.flatten() {
        let per_tick = raw.to_ascii_lowercase().contains("per tick");
        let key = match opcode.as_str() {
            "STR" | "STA" | "AGI" | "DEX" | "WIS" | "INT" | "CHA" | "AC" | "ATK" => opcode,
            "HP" => if per_tick { "HP REGEN".into() } else { "HP".into() },
            "MANA" => if per_tick { "MANA REGEN".into() } else { "MANA".into() },
            "RESIST_MAGIC" => "SV MAGIC".into(),
            "RESIST_FIRE" => "SV FIRE".into(),
            "RESIST_COLD" => "SV COLD".into(),
            "RESIST_POISON" => "SV POISON".into(),
            "RESIST_DISEASE" => "SV DISEASE".into(),
            _ => continue, // counters, summons, movement, … — not character stats
        };
        out.entry(sid).or_default().push((key, amount));
    }
    out
}

/// (level, class) -> (hp, hp_fac, mana, mana_fac) — the community base curves
/// (import_stat_estimator.py). Empty on DBs without the import; the engine then
/// falls back to the thought-experiment placeholder.
fn load_class_base_curve(c: &Connection) -> BTreeMap<(u32, String), (f64, f64, f64, f64)> {
    let mut out = BTreeMap::new();
    if !table_exists(c, "class_base_curve") {
        return out;
    }
    if let Ok(mut stmt) =
        c.prepare("SELECT level, class, hp, hp_fac, mana, mana_fac FROM class_base_curve")
    {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok(((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?),
                (r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, r.get::<_, f64>(4)?,
                 r.get::<_, f64>(5)?)))
        }) {
            for (k, v) in rows.flatten() {
                out.insert(k, v);
            }
        }
    }
    out
}

fn build_snapshot() -> Snapshot {
    let c = conn().expect("cannot open wiki db");
    let names = load_spell_names(&c).unwrap_or_default();
    let clevels = load_spell_class_levels(&c).unwrap_or_default();
    Snapshot {
        items_by_id: load_items_full(&c).unwrap_or_default(),
        buff_lines: load_buff_lines(&c).unwrap_or_default(),
        pet_summons: load_pet_summons(&c, &names, &clevels),
        combinations: load_combinations(&c),
        race_base_stats: load_race_stats(&c),
        class_stat_mods: load_class_mods(&c),
        formulas: crate::builds::load_formulas().unwrap_or_default(),
        item_effects: load_item_effects(&c),
        pet_slot_rule: load_pet_slot_rule(),
        aas: load_aas(&c),
        spell_durations: load_spell_durations(&c),
        spell_stat_effects: load_spell_stat_effects(&c),
        class_base_curve: load_class_base_curve(&c),
        class_levels: clevels,
        spell_names: names,
    }
}

/// Process-wide snapshot. Heavy wiki data is immutable, but the editable formula rows
/// live in it too — so Settings edits (and a future re-sync) call `refresh_snapshot`.
/// Arc keeps the swap cheap for readers already holding the old one.
static SNAPSHOT: OnceLock<RwLock<Arc<Snapshot>>> = OnceLock::new();

fn cell() -> &'static RwLock<Arc<Snapshot>> {
    SNAPSHOT.get_or_init(|| RwLock::new(Arc::new(build_snapshot())))
}

pub fn snapshot() -> Arc<Snapshot> {
    cell().read().expect("snapshot lock poisoned").clone()
}

/// Rebuild from SQLite (after editing formulas / re-syncing the wiki data).
pub fn refresh_snapshot() {
    let fresh = Arc::new(build_snapshot());
    *cell().write().expect("snapshot lock poisoned") = fresh;
}

// ------------------------------------------------------------------ page queries
/// Slim item list for the gear browser (kept from M1; filter by classes).
pub fn load_items(classes: &[String]) -> rusqlite::Result<Vec<Item>> {
    let c = conn()?;
    let all = load_items_full(&c)?;
    let mut items: Vec<Item> = all.into_values().collect();
    // Non-canonical entries stay out of the browser unless deliberately revealed.
    // Filtered here rather than in load_items_full so a saved build that already
    // equips one still resolves instead of reporting an unknown item.
    if !eql_data::is_pickle_wizard(classes) {
        items.retain(|it| !it.non_canonical);
    }
    if !classes.is_empty() {
        items.retain(|it| eql_engine::item_wearable_by(it, classes));
    }
    Ok(items)
}

#[derive(serde::Serialize)]
pub struct SpellRow {
    pub id: i64,
    pub name: String,
    pub class: String,
    pub required_class_level: u32,
    pub is_autogranted: bool,
    pub target_type: Option<String>,
    pub spell_type: Option<String>,
    pub is_beneficial: bool,
    pub is_song: bool,
    pub role: Option<String>,
    pub era: Option<String>,
    pub mana: Option<i64>,
    pub duration: Option<String>,
    /// seconds; tier scaling (-4%/tier) is applied in the UI, formula-driven
    pub casting_time: Option<f64>,
    pub recast_time: Option<f64>,
    pub icon: Option<String>,
    pub is_summon: bool,
    /// client-extracted ranges (eqlbuilds spell_client; PARTIALLY_VERIFIED — the
    /// server can override): the trusted base for damage/heal tier math
    pub dmg_min: Option<i64>,
    pub dmg_max: Option<i64>,
    pub heal_min: Option<i64>,
    pub heal_max: Option<i64>,
    /// client-resolved description ("causing between 700 and 808 damage")
    pub resolved_description: Option<String>,
}

/// Spells page: everything any of the build's classes gets at or below `level` (plan §2).
pub fn query_spells(classes: &[String], level: u32) -> rusqlite::Result<Vec<SpellRow>> {
    let c = conn()?;
    let ph = classes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    // client-extracted columns degrade to NULL on DBs where the eqlbuilds import
    // hasn't run; client mana/cast also FILL wiki gaps (COALESCE). scf = the EQL
    // client's own spells_us.txt (import_spell_client_file.py) — most authoritative
    // for base mechanics, so it fills gaps ahead of eqlbuilds; both stay behind the
    // wiki value (fill-gaps only, never override — name-collision + server-rebalance risk).
    let has_client = table_exists(&c, "spell_client");
    let has_file = table_exists(&c, "spell_client_file");
    let mana_expr = match (has_file, has_client) {
        (true, true) => "COALESCE(s.mana, scf.mana, sc.mana)",
        (true, false) => "COALESCE(s.mana, scf.mana)",
        (false, true) => "COALESCE(s.mana, sc.mana)",
        (false, false) => "s.mana",
    };
    let cast_expr = match (has_file, has_client) {
        (true, true) => "COALESCE(s.casting_time, scf.cast_ms / 1000.0, sc.cast_ms / 1000.0)",
        (true, false) => "COALESCE(s.casting_time, scf.cast_ms / 1000.0)",
        (false, true) => "COALESCE(s.casting_time, sc.cast_ms / 1000.0)",
        (false, false) => "s.casting_time",
    };
    let recast_expr = if has_file {
        "COALESCE(s.recast_time, scf.recast_ms / 1000.0)"
    } else {
        "s.recast_time"
    };
    let file_join = if has_file {
        "LEFT JOIN spell_client_file scf ON scf.spell_id = s.id"
    } else {
        ""
    };
    // damage/heal: eqlbuilds first (already displayed), client-file decode fills the
    // ~282 spells eqlbuilds lacks (validated 258/260 == eqlbuilds on the overlap).
    let (dmin, dmax, hmin, hmax) = if has_file {
        ("COALESCE(sc.dmg_min, scf.dmg_min)", "COALESCE(sc.dmg_max, scf.dmg_max)",
         "COALESCE(sc.heal_min, scf.heal_min)", "COALESCE(sc.heal_max, scf.heal_max)")
    } else {
        ("sc.dmg_min", "sc.dmg_max", "sc.heal_min", "sc.heal_max")
    };
    let (client_cols, client_join) = if has_client {
        (format!(
            "{mana_expr}, {cast_expr}, \
             {dmin}, {dmax}, {hmin}, {hmax}, sc.resolved_description"
        ),
         "LEFT JOIN spell_client sc ON sc.spell_id = s.id")
    } else if has_file {
        // no eqlbuilds table, but the client-file decode still provides dmg/heal
        (format!(
            "{mana_expr}, {cast_expr}, \
             scf.dmg_min, scf.dmg_max, scf.heal_min, scf.heal_max, NULL"
        ), "")
    } else {
        (format!("{mana_expr}, {cast_expr}, NULL, NULL, NULL, NULL, NULL"), "")
    };
    let sql = format!(
        "SELECT s.id, COALESCE(s.page_title, s.name), cl.abbr, l.required_class_level, \
                l.is_autogranted, s.target_type_raw, s.spell_type_raw, s.is_beneficial, \
                s.is_song, s.role, s.era, s.duration_raw, s.icon, \
                EXISTS(SELECT 1 FROM spell_pet_summon p WHERE p.spell_id = s.id), \
                {recast_expr}, {client_cols} \
         FROM spell s JOIN spell_class_level l ON l.spell_id = s.id \
         JOIN class cl ON cl.id = l.class_id {file_join} {client_join} \
         WHERE cl.abbr IN ({ph}) AND l.required_class_level <= ? AND s.is_npc_only = 0 \
         ORDER BY l.required_class_level, s.name"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = classes
        .iter()
        .map(|s| Box::new(s.to_uppercase()) as Box<dyn rusqlite::ToSql>)
        .collect();
    params.push(Box::new(level));
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())),
        |r| {
            Ok(SpellRow {
                id: r.get(0)?, name: r.get(1)?, class: r.get(2)?,
                required_class_level: r.get(3)?,
                is_autogranted: r.get::<_, i64>(4)? != 0,
                target_type: r.get(5)?, spell_type: r.get(6)?,
                is_beneficial: r.get::<_, i64>(7)? != 0,
                is_song: r.get::<_, i64>(8)? != 0,
                role: r.get(9)?, era: r.get(10)?, duration: r.get(11)?,
                icon: r.get(12)?, is_summon: r.get::<_, i64>(13)? != 0,
                recast_time: r.get(14)?,
                mana: r.get(15)?, casting_time: r.get(16)?,
                dmg_min: r.get(17)?, dmg_max: r.get(18)?,
                heal_min: r.get(19)?, heal_max: r.get(20)?,
                resolved_description: r.get(21)?,
            })
        },
    )?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// One combat mode (stance or invocation) from the eqlbuilds.com snapshot. The
/// descriptions carry the real numbers ("+100% melee damage…"); display-only v1 —
/// nothing feeds the stat pipeline until measured stacking rules exist.
#[derive(serde::Serialize)]
pub struct Mode {
    pub id: String,
    pub kind: String, // "stance" | "invocation"
    pub name: String,
    pub message: Option<String>,
    pub description: Option<String>,
}

/// Stances + invocations, window order as shipped. Empty when the import hasn't run.
pub fn list_modes() -> rusqlite::Result<Vec<Mode>> {
    let c = conn()?;
    let mut out = Vec::new();
    for (table, kind) in [("stance", "stance"), ("invocation", "invocation")] {
        if !table_exists(&c, table) {
            continue;
        }
        let mut stmt =
            c.prepare(&format!("SELECT id, name, message, description FROM {table} ORDER BY rowid"))?;
        for row in stmt.query_map([], |r| {
            Ok(Mode {
                id: r.get(0)?, kind: kind.to_string(), name: r.get(1)?,
                message: r.get(2)?, description: r.get(3)?,
            })
        })? {
            out.push(row?);
        }
    }
    Ok(out)
}

/// One merged skill line for the build: multiclass combine is BEST_OF (plan §4.7
/// formula multi_class_skill_combine) — highest cap wins, earliest training level shown.
#[derive(serde::Serialize)]
pub struct SkillRow {
    pub name: String,
    pub trained_at: Option<i64>, // earliest level any build class trains it
    pub cap: Option<i64>,        // best cap across the build's classes
    pub best_class: Option<String>, // which class grants that cap
    pub classes: Vec<String>,    // every build class that has the skill
}

/// Skills for the build's classes, merged BEST_OF (eqlbuilds class_skill snapshot).
pub fn query_skills(classes: &[String]) -> rusqlite::Result<Vec<SkillRow>> {
    let c = conn()?;
    if classes.is_empty() || !table_exists(&c, "class_skill") {
        return Ok(Vec::new());
    }
    let ph = classes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = c.prepare(&format!(
        "SELECT name, class_abbr, trained_at, cap FROM class_skill \
         WHERE class_abbr IN ({ph}) ORDER BY name, class_abbr"
    ))?;
    let params: Vec<String> = classes.iter().map(|s| s.to_uppercase()).collect();
    let mut merged: BTreeMap<String, SkillRow> = BTreeMap::new();
    for row in stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?, r.get::<_, Option<i64>>(3)?))
    })? {
        let (name, abbr, trained, cap) = row?;
        let e = merged.entry(name.clone()).or_insert(SkillRow {
            name, trained_at: None, cap: None, best_class: None, classes: Vec::new(),
        });
        e.classes.push(abbr.clone());
        if trained.is_some() && (e.trained_at.is_none() || trained < e.trained_at) {
            e.trained_at = trained;
        }
        if cap.is_some() && (e.cap.is_none() || cap > e.cap) {
            e.cap = cap;
            e.best_class = Some(abbr);
        }
    }
    Ok(merged.into_values().collect())
}

/// Everything the `?` popup shows about one spell: mechanics, stacking membership,
/// and ACQUISITION (vendor/NPC/zone rows from the wiki). Acquisition may be empty —
/// the UI then says it has not been confirmed for EQL rather than inventing legacy data.
#[derive(serde::Serialize)]
pub struct SpellSourceRow {
    pub source_type: String, // VENDOR | DROP | QUEST | RESEARCH | CLASS_VENDOR | NOTE
    pub zone: Option<String>,
    pub npc: Option<String>,
    pub area: Option<String>,
    pub loc: Option<String>,
    /// which class's guild vendor this is (CLASS_VENDOR rows, from the purchase dataset)
    pub class_source: Option<String>,
    /// free text: research components, or a verbatim wiki note
    pub raw_text: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SpellInfo {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub mana: Option<i64>,
    pub casting_time: Option<f64>,
    pub recast_time: Option<f64>,
    pub duration: Option<String>,
    pub target_type: Option<String>,
    pub resist_type: Option<String>,
    pub era: Option<String>,
    pub is_song: bool,
    /// (class abbr, level, autogranted)
    pub class_levels: Vec<(String, u32, bool)>,
    /// buff lines this spell participates in (stacking family)
    pub buff_lines: Vec<String>,
    pub sources: Vec<SpellSourceRow>,
    /// scroll-carrying items (spell_item_source)
    pub item_sources: Vec<String>,
    /// eqlwiki page for the spell
    pub wiki_url: String,
}

pub fn spell_info(id: i64) -> rusqlite::Result<SpellInfo> {
    let c = conn()?;
    let (name, title, desc, mana, cast, recast, dur, tgt, resist, era, song): (
        String, Option<String>, Option<String>, Option<i64>, Option<f64>, Option<f64>,
        Option<String>, Option<String>, Option<String>, Option<String>, i64,
    ) = c.query_row(
        "SELECT COALESCE(page_title, name), page_title, description, mana, casting_time, \
                recast_time, duration_raw, target_type_raw, resist_type, era, is_song \
         FROM spell WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)),
    )?;
    let mut class_levels = Vec::new();
    let mut stmt = c.prepare(
        "SELECT cl.abbr, l.required_class_level, l.is_autogranted \
         FROM spell_class_level l JOIN class cl ON cl.id = l.class_id \
         WHERE l.spell_id=?1 ORDER BY l.required_class_level, cl.abbr",
    )?;
    for row in stmt.query_map([id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, i64>(2)? != 0))
    })? {
        class_levels.push(row?);
    }
    let mut buff_lines = Vec::new();
    if table_exists(&c, "buff_line_member") {
        let mut ls = c.prepare(
            "SELECT DISTINCT bl.name FROM buff_line_member m \
             JOIN buff_line bl ON bl.id = m.buff_line_id WHERE m.spell_id=?1",
        )?;
        for row in ls.query_map([id], |r| r.get::<_, String>(0))? {
            buff_lines.push(row?);
        }
    }
    let mut sources = Vec::new();
    if table_exists(&c, "spell_source") {
        let has_class = c
            .prepare("SELECT class_source FROM spell_source LIMIT 1")
            .map(|_| true)
            .unwrap_or(false);
        let class_col = if has_class { "class_source" } else { "NULL" };
        // explicit-page vendors first (high confidence), then class-vendor, then the rest
        let mut ss = c.prepare(&format!(
            "SELECT source_type, zone_name, npc_name, area, loc, {class_col}, raw_text \
             FROM spell_source WHERE spell_id=?1 AND source_type != 'UNKNOWN' \
             ORDER BY CASE source_type WHEN 'VENDOR' THEN 0 WHEN 'CLASS_VENDOR' THEN 1 \
                                        WHEN 'RESEARCH' THEN 2 ELSE 3 END, id"
        ))?;
        for row in ss.query_map([id], |r| {
            Ok(SpellSourceRow {
                source_type: r.get(0)?, zone: r.get(1)?, npc: r.get(2)?,
                area: r.get(3)?, loc: r.get(4)?, class_source: r.get(5)?, raw_text: r.get(6)?,
            })
        })? {
            sources.push(row?);
        }
    }
    let mut item_sources = Vec::new();
    if table_exists(&c, "spell_item_source") {
        let mut is_ = c.prepare(
            "SELECT item_name FROM spell_item_source WHERE spell_id=?1 ORDER BY item_name",
        )?;
        for row in is_.query_map([id], |r| r.get::<_, String>(0))? {
            item_sources.push(row?);
        }
    }
    let wiki_url = format!(
        "https://eqlwiki.com/{}",
        title.as_deref().unwrap_or(&name).replace(' ', "_")
    );
    Ok(SpellInfo {
        id, name, description: desc, mana, casting_time: cast, recast_time: recast,
        duration: dur, target_type: tgt, resist_type: resist, era, is_song: song != 0,
        class_levels, buff_lines, sources, item_sources, wiki_url,
    })
}

/// One item carrying a FOCUS effect, with its drop sources — the Focus Effects
/// reference tab. Grouping/tier parsing happens in the UI.
#[derive(serde::Serialize)]
pub struct FocusEffectRow {
    pub effect_name: String, // "Burning Affliction III"
    /// the focus spell's own wiki description, when the spell page exists
    pub description: Option<String>,
    pub item_pageid: i64,
    pub item_name: String,
    pub item_classes: Vec<String>,
    pub era: Option<String>,
    /// (mob, zone) drop sources; empty = quest/unknown (the wiki link has the rest)
    pub sources: Vec<(String, Option<String>)>,
}

pub fn focus_effects() -> rusqlite::Result<Vec<FocusEffectRow>> {
    let c = conn()?;
    let mut stmt = c.prepare(
        "SELECT ie.effect_name, sp.description, i.pageid, i.name, i.era \
         FROM item_effect ie \
         JOIN items i ON i.pageid = ie.pageid \
         LEFT JOIN spell sp ON COALESCE(sp.page_title, sp.name) = ie.effect_name \
         WHERE ie.activation_type = 'FOCUS' \
         ORDER BY ie.effect_name, i.name",
    )?;
    let mut out: Vec<FocusEffectRow> = stmt
        .query_map([], |r| {
            Ok(FocusEffectRow {
                effect_name: r.get(0)?, description: r.get(1)?,
                item_pageid: r.get(2)?, item_name: r.get(3)?, era: r.get(4)?,
                item_classes: Vec::new(), sources: Vec::new(),
            })
        })?
        .filter_map(|x| x.ok())
        .collect();
    let mut cstmt = c.prepare("SELECT class FROM item_classes WHERE pageid=?1 ORDER BY rowid")?;
    // drops join folds backticks on both sides (same rule as farm_sources)
    let mut dstmt = c.prepare(
        "SELECT d.mob_name, m.zone FROM drops d LEFT JOIN mobs m ON m.name = d.mob_name \
         WHERE replace(d.item_name, char(96), char(39)) = replace(?1, char(96), char(39)) \
         ORDER BY m.zone, d.mob_name",
    )?;
    for row in out.iter_mut() {
        row.item_classes = cstmt
            .query_map([row.item_pageid], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        row.sources = dstmt
            .query_map([&row.item_name], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .filter_map(|x| x.ok())
            .collect();
    }
    Ok(out)
}

/// One row of the Exaltations reference tab (mirrors FocusEffectRow): an item whose
/// FOCUS/CLICK/WORN/PROC effect can be extracted at +4 into "<item> (Exaltation)".
/// Regen effects are excluded — not extractable so far (same rule as the augment
/// catalog in eql-engine::augments).
#[derive(serde::Serialize)]
pub struct ExaltationRow {
    pub kind: String, // FOCUS | CLICK | WORN | PROC
    pub effect_name: String,
    /// the effect spell's own wiki description, when the spell page exists
    pub description: Option<String>,
    /// the effect's activation level requirement on this item (game "Req Level N")
    pub required_level: Option<i64>,
    pub item_pageid: i64,
    pub item_name: String,
    pub item_classes: Vec<String>,
    pub era: Option<String>,
    /// (mob, zone) drop sources; empty = quest/unknown (the wiki link has the rest)
    pub sources: Vec<(String, Option<String>)>,
    /// the effect spell's parsed mechanical lines (spell_effect.raw_text — what the
    /// effect actually does, e.g. "Increases your faction with … by 250")
    pub effect_lines: Vec<String>,
}

pub fn exaltation_effects() -> rusqlite::Result<Vec<ExaltationRow>> {
    let c = conn()?;
    let mut stmt = c.prepare(
        "SELECT ie.activation_type, ie.effect_name, sp.description, ie.required_level, \
                i.pageid, i.name, i.era, sp.id \
         FROM item_effect ie \
         JOIN items i ON i.pageid = ie.pageid \
         LEFT JOIN spell sp ON COALESCE(sp.page_title, sp.name) = ie.effect_name \
         WHERE ie.activation_type IN ('FOCUS','CLICK','WORN','PROC') \
         ORDER BY ie.activation_type, ie.effect_name, i.name",
    )?;
    let mut out: Vec<(Option<i64>, ExaltationRow)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<i64>>(7)?,
                ExaltationRow {
                    kind: r.get(0)?, effect_name: r.get(1)?, description: r.get(2)?,
                    required_level: r.get(3)?, item_pageid: r.get(4)?, item_name: r.get(5)?,
                    era: r.get(6)?, item_classes: Vec::new(), sources: Vec::new(),
                    effect_lines: Vec::new(),
                },
            ))
        })?
        .filter_map(|x| x.ok())
        // regen is not extractable (so far) — keep the reference honest
        .filter(|(_, r)| !r.effect_name.to_ascii_lowercase().contains("regen"))
        .collect();
    let mut cstmt = c.prepare("SELECT class FROM item_classes WHERE pageid=?1 ORDER BY rowid")?;
    let mut dstmt = c.prepare(
        "SELECT d.mob_name, m.zone FROM drops d LEFT JOIN mobs m ON m.name = d.mob_name \
         WHERE replace(d.item_name, char(96), char(39)) = replace(?1, char(96), char(39)) \
         ORDER BY m.zone, d.mob_name",
    )?;
    // effect lines per SPELL, fetched once each (many items share one effect)
    let mut lines_by_spell: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    let has_fx = table_exists(&c, "spell_effect");
    let mut estmt = if has_fx {
        Some(c.prepare(
            "SELECT raw_text FROM spell_effect \
             WHERE spell_id=?1 AND raw_text IS NOT NULL AND raw_text != '' \
             ORDER BY slot_number",
        )?)
    } else {
        None
    };
    for (spell_id, row) in out.iter_mut() {
        row.item_classes = cstmt
            .query_map([row.item_pageid], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        row.sources = dstmt
            .query_map([&row.item_name], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .filter_map(|x| x.ok())
            .collect();
        if let (Some(sid), Some(estmt)) = (*spell_id, estmt.as_mut()) {
            let lines = match lines_by_spell.get(&sid) {
                Some(l) => l.clone(),
                None => {
                    let l: Vec<String> = estmt
                        .query_map([sid], |r| r.get::<_, String>(0))?
                        .filter_map(|x| x.ok())
                        .collect();
                    lines_by_spell.insert(sid, l.clone());
                    l
                }
            };
            row.effect_lines = lines;
        }
    }
    Ok(out.into_iter().map(|(_, r)| r).collect())
}

/// One item/consumable-sourced buff-line member for the "Add Other Buff" catalog
/// (clickies, worn effects, procs, potions — the sources that NEVER auto-apply).
#[derive(serde::Serialize)]
pub struct OtherBuffRow {
    /// display name — matches the resolver's member naming, so enabling by this name
    /// lines up with build.other_buffs
    pub name: String,
    pub line: String,
    pub source_kind: String, // CLICK | PROC | WORN | CONSUMABLE
    pub value: Option<f64>,
    pub source_items: Option<String>,
}

pub fn list_other_buffs() -> rusqlite::Result<Vec<OtherBuffRow>> {
    let c = conn()?;
    if !table_exists(&c, "buff_line_member") {
        return Ok(Vec::new());
    }
    let mut stmt = c.prepare(
        "SELECT DISTINCT COALESCE(s.page_title, s.name, m.member_name_raw), bl.name, \
                m.source_kind, m.value_base, m.source_items \
         FROM buff_line_member m \
         JOIN buff_line bl ON bl.id = m.buff_line_id \
         LEFT JOIN spell s ON s.id = m.spell_id \
         WHERE m.source_kind IN ('CLICK','PROC','WORN','CONSUMABLE') \
         ORDER BY m.source_kind, bl.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(OtherBuffRow {
            name: r.get::<_, Option<String>>(0)?.unwrap_or_else(|| "?".into()),
            line: r.get(1)?, source_kind: r.get(2)?, value: r.get(3)?,
            source_items: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).filter(|r| r.name != "?").collect())
}

/// Spell ids that can actually be RECEIVED from another player: a SPELL-kind buff-line
/// member that is not self-only. The Spells tab's "Cast" button is gated on this —
/// everything else (self-only, or in no known buff line) can never enter the plan.
pub fn external_receivable() -> rusqlite::Result<Vec<i64>> {
    let c = conn()?;
    if !table_exists(&c, "buff_line_member") {
        return Ok(Vec::new());
    }
    let mut stmt = c.prepare(
        "SELECT DISTINCT spell_id FROM buff_line_member \
         WHERE spell_id IS NOT NULL AND source_kind='SPELL' \
           AND (is_self_only IS NULL OR is_self_only=0)",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// spell pageid -> buff line name (family grouping for the spellbook auto-organizer).
/// Only buff spells have wiki line data; a spell in several lines gets its first line.
pub fn spell_line_map() -> rusqlite::Result<BTreeMap<i64, String>> {
    let c = conn()?;
    if !table_exists(&c, "buff_line_member") {
        return Ok(BTreeMap::new());
    }
    let mut stmt = c.prepare(
        "SELECT m.spell_id, bl.name FROM buff_line_member m \
         JOIN buff_line bl ON bl.id = m.buff_line_id \
         WHERE m.spell_id IS NOT NULL ORDER BY m.spell_id, bl.id",
    )?;
    let mut out: BTreeMap<i64, String> = BTreeMap::new();
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })? {
        let (sid, line) = row?;
        out.entry(sid).or_insert(line); // first line wins
    }
    Ok(out)
}

/// What a linked effect spell actually does — the hover explanation for item/augment
/// effects (Stats page + item windows). `effects` are the wiki's own parsed lines
/// ("Increase Attack Speed by 64%"), shown verbatim.
#[derive(serde::Serialize)]
pub struct SpellDetails {
    pub id: i64,
    pub name: String,
    pub target_type: Option<String>,
    pub duration: Option<String>,
    pub mana: Option<i64>,
    pub effects: Vec<String>,
}

/// Bulk lookup for effect hover cards (spell ids from item_effect.spell_id).
pub fn spell_details(ids: &[i64]) -> rusqlite::Result<BTreeMap<i64, SpellDetails>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let c = conn()?;
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = c.prepare(&format!(
        "SELECT id, COALESCE(page_title, name), target_type_raw, duration_raw, mana \
         FROM spell WHERE id IN ({ph})"
    ))?;
    let mut out: BTreeMap<i64, SpellDetails> = stmt
        .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok(SpellDetails {
                id: r.get(0)?, name: r.get(1)?, target_type: r.get(2)?,
                duration: r.get(3)?, mana: r.get(4)?, effects: Vec::new(),
            })
        })?
        .filter_map(|x| x.ok())
        .map(|d| (d.id, d))
        .collect();
    if table_exists(&c, "spell_effect") {
        let mut estmt = c.prepare(&format!(
            "SELECT spell_id, raw_text FROM spell_effect \
             WHERE spell_id IN ({ph}) AND raw_text IS NOT NULL AND raw_text != '' \
             ORDER BY spell_id, slot_number"
        ))?;
        for row in estmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })? {
            let (sid, line) = row?;
            if let Some(d) = out.get_mut(&sid) {
                d.effects.push(line);
            }
        }
    }
    Ok(out)
}

/// One structured spell_effect row of a FOCUS spell — the Spellbook's focus math
/// (percentages + limit lines). Kept raw-ish; the frontend classifies and applies.
#[derive(serde::Serialize)]
pub struct FocusDetailRow {
    pub spell_id: i64,
    pub opcode: String,
    pub base_amount: Option<f64>,
    pub max_amount: Option<f64>,
    pub raw_text: String,
}

pub fn focus_details(ids: &[i64]) -> rusqlite::Result<Vec<FocusDetailRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let c = conn()?;
    if !table_exists(&c, "spell_effect") {
        return Ok(Vec::new());
    }
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = c.prepare(&format!(
        "SELECT spell_id, COALESCE(opcode,''), base_amount, max_amount, COALESCE(raw_text,'') \
         FROM spell_effect WHERE spell_id IN ({ph}) ORDER BY spell_id, slot_number"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok(FocusDetailRow {
            spell_id: r.get(0)?, opcode: r.get(1)?, base_amount: r.get(2)?,
            max_amount: r.get(3)?, raw_text: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Structured focus effect for the Spellbook math, decoded from the CLIENT effect slots
/// (spell_client_effect) rather than parsed from wiki prose — so its limits are exact.
/// SPA map (grounded against our focus effects 2026-07-21): type 124=DMG 125=HEAL
/// 127=HASTE 128=DURATION 129=RANGE 131=REAGENT 132=MANA; limits 134=Max Level(+decay)
/// 138=spell type 140=Min Duration 141/142=Min Level.
#[derive(serde::Serialize, Default)]
pub struct FocusClient {
    pub spell_id: i64,
    pub kind: String, // DMG | HEAL | HASTE | MANA | DURATION | RANGE | REAGENT
    pub pct_min: i64,
    pub pct_max: i64,
    pub max_level: Option<i64>,       // full effect up to this level of the focused spell
    pub level_decay_pct: Option<i64>, // % of the effect lost per level beyond max_level
    pub min_level: Option<i64>,       // focused spell must be at least this level
    pub min_duration_ticks: Option<i64>,
    pub beneficial_only: bool,
    pub detrimental_only: bool,
}

pub fn focus_client(ids: &[i64]) -> rusqlite::Result<Vec<FocusClient>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let c = conn()?;
    if !table_exists(&c, "spell_client_effect") {
        return Ok(Vec::new());
    }
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = c.prepare(&format!(
        "SELECT spell_id, spa, base, base2, max FROM spell_client_effect \
         WHERE spell_id IN ({ph}) ORDER BY spell_id, slot"
    ))?;
    let mut by_spell: BTreeMap<i64, Vec<(i64, i64, i64, i64)>> = BTreeMap::new();
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
    })?;
    for row in rows.flatten() {
        by_spell.entry(row.0).or_default().push((row.1, row.2, row.3, row.4));
    }
    let kind_of = |spa: i64| match spa {
        124 => Some("DMG"),
        125 => Some("HEAL"),
        127 => Some("HASTE"),
        128 => Some("DURATION"),
        129 => Some("RANGE"),
        131 => Some("REAGENT"),
        132 => Some("MANA"),
        _ => None,
    };
    let mut out = Vec::new();
    for (spell_id, slots) in by_spell {
        // the type slot (first recognized focus SPA); everything else is a limit
        let Some((kind, base, base2)) = slots
            .iter()
            .find_map(|(spa, b, b2, _m)| kind_of(*spa).map(|k| (k, *b, *b2)))
        else {
            continue;
        };
        let mut f = FocusClient {
            spell_id,
            kind: kind.to_string(),
            pct_min: base.abs(),
            pct_max: if base2 != 0 { base2.abs() } else { base.abs() },
            ..Default::default()
        };
        for (spa, b, b2, _m) in &slots {
            match spa {
                // 253/255 in max-level base is the "no limit" sentinel
                134 if *b > 0 && *b < 253 => {
                    f.max_level = Some(*b);
                    if *b2 != 0 {
                        f.level_decay_pct = Some(b2.abs());
                    }
                }
                141 | 142 if *b > 0 => f.min_level = Some(*b),
                140 if *b > 0 => f.min_duration_ticks = Some(*b),
                138 => {
                    if *b == 1 {
                        f.beneficial_only = true;
                    } else if *b == 0 {
                        f.detrimental_only = true;
                    }
                }
                _ => {}
            }
        }
        out.push(f);
    }
    Ok(out)
}

/// spell pageid -> icon token (for spellbook gem squares).
pub fn spell_icons(ids: &[i64]) -> rusqlite::Result<BTreeMap<i64, String>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let c = conn()?;
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut stmt = c.prepare(&format!(
        "SELECT id, icon FROM spell WHERE id IN ({ph}) AND icon IS NOT NULL"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

#[derive(serde::Serialize)]
pub struct FarmSource {
    pub item: String,
    pub mob: String,
    pub rarity: Option<String>,
    pub zone: Option<String>,
    pub mob_level: Option<String>,
}

/// Farm list: where do these items drop? (drops join mobs by name — v1 schema.)
pub fn farm_sources(item_names: &[String]) -> rusqlite::Result<Vec<FarmSource>> {
    if item_names.is_empty() {
        return Ok(Vec::new());
    }
    let c = conn()?;
    let ph = item_names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    // fold backtick->apostrophe on BOTH sides: mob pages write `Gharn`s`, item pages `Gharn's`
    let sql = format!(
        "SELECT d.item_name, d.mob_name, d.rarity, m.zone, m.level \
         FROM drops d LEFT JOIN mobs m ON m.name = d.mob_name \
         WHERE replace(d.item_name, char(96), char(39)) IN ({ph}) \
         ORDER BY d.item_name, d.mob_name"
    );
    let folded: Vec<String> = item_names.iter().map(|n| n.replace('`', "'")).collect();
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(folded.iter()),
        |r| {
            Ok(FarmSource {
                item: r.get(0)?, mob: r.get(1)?, rarity: r.get(2)?,
                zone: r.get(3)?, mob_level: r.get(4)?,
            })
        },
    )?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

#[derive(serde::Serialize)]
pub struct StaticData {
    pub classes: Vec<String>,
    pub races: Vec<String>,
    pub paperdoll_slots: Vec<String>,
    pub pet_summons: Vec<PetSummonInfo>,
    /// 0 items = the wiki db was not found or is empty; UI must show a hard error
    pub item_count: usize,
    pub wiki_db_path: String,
    /// eras present in the item data, in ERA_ORDER (expansion toggle chips)
    pub eras: Vec<String>,
    /// eras live in the game today (wiki Template:PageEra) — the app's default set
    pub default_enabled_eras: Vec<String>,
    /// live level cap; the slider still allows higher for future planning
    pub default_level_cap: u32,
    /// +N gate for Exaltation extraction (formula exaltation_extract_min_tier)
    pub exaltation_extract_min_tier: u32,
    /// item upgrade %/tier (formula item_tier_scaling_pct) — the UI's tierBonus must
    /// use THIS so tooltips agree with the engine's stat totals after a Settings edit
    pub item_tier_scaling_pct: f64,
    /// spell tier scaling (community-reconstructed 2026-07; all Settings-editable):
    /// dmg/heal +pct/tier linear floor; mana -pct/tier PROVISIONAL (skip below the
    /// floor); cast/recovery/reuse -pct/tier; reagent conserve chance +pct/tier
    pub spell_tier_scaling_pct: f64,
    pub spell_tier_mana_pct: f64,
    pub spell_tier_mana_floor: i64,
    pub spell_tier_cast_pct: f64,
    pub reagent_conserve_pct_per_tier: f64,
    /// starting-attribute tables (client-validated: each class sums to exactly 30,
    /// additive with race base — eqltools.com Attributes) for the Overview panel
    pub race_base_stats: BTreeMap<String, BTreeMap<String, i64>>,
    pub class_stat_mods: BTreeMap<String, BTreeMap<String, i64>>,
    /// naked per-attribute ceiling players report (formula stat_naked_ceiling)
    pub stat_naked_ceiling: i64,
    /// buffed per-attribute hard cap (formula stat_cap; the Stats tab's Cap column)
    pub stat_cap: i64,
    /// buffed resist/save hard cap (formula resist_cap; applies to the SV_* stats)
    pub resist_cap: i64,
}

pub fn static_data() -> StaticData {
    let snap = snapshot();
    let f = |key: &str, default: f64| -> f64 {
        snap.formulas.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    };
    let present: std::collections::BTreeSet<&str> = snap
        .items_by_id
        .values()
        .filter_map(|i| i.era.as_deref())
        .collect();
    StaticData {
        item_count: snap.items_by_id.len(),
        wiki_db_path: wiki_db_path().display().to_string(),
        eras: eql_data::ERA_ORDER
            .iter()
            .filter(|e| present.contains(**e))
            .map(|e| e.to_string())
            .collect(),
        // only offer defaults for eras that actually tag data in this mirror
        default_enabled_eras: eql_data::DEFAULT_ENABLED_ERAS
            .iter()
            .filter(|e| present.contains(**e))
            .map(|e| e.to_string())
            .collect(),
        default_level_cap: eql_data::DEFAULT_LEVEL_CAP,
        exaltation_extract_min_tier: eql_engine::augments::extract_min_tier(&snap),
        item_tier_scaling_pct: f("item_tier_scaling_pct", 10.0),
        race_base_stats: snap.race_base_stats.clone(),
        class_stat_mods: snap.class_stat_mods.clone(),
        stat_naked_ceiling: f("stat_naked_ceiling", 150.0) as i64,
        stat_cap: f("stat_cap", 510.0) as i64,
        resist_cap: f("resist_cap", 1000.0) as i64,
        spell_tier_scaling_pct: f("spell_tier_scaling_pct", 6.0),
        spell_tier_mana_pct: f("spell_tier_mana_pct", 6.0),
        spell_tier_mana_floor: f("spell_tier_mana_floor", 20.0) as i64,
        spell_tier_cast_pct: f("spell_tier_cast_pct", 4.0),
        reagent_conserve_pct_per_tier: f("reagent_conserve_pct_per_tier", 10.0),
        classes: ["WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM",
                  "NEC", "WIZ", "MAG", "ENC", "BST", "BER"]
            .iter().map(|s| s.to_string()).collect(),
        races: snap.race_base_stats.keys().cloned().collect(),
        paperdoll_slots: eql_data::PAPERDOLL_SLOTS.iter().map(|s| s.to_string()).collect(),
        pet_summons: snap.pet_summons.values().cloned().collect(),
    }
}
