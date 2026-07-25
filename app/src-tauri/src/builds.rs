//! builds.db — the PRECIOUS database (plan §2.2): %LOCALAPPDATA%/EQLBuilder/builds.db.
//! Owns builds + the editable formula_table. Soft refs (pageid + name) to wiki data.
use eql_data::BuildInput;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn builds_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("EQL_BUILDS_DB") {
        return PathBuf::from(p);
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join("EQLBuilder");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("builds.db")
}

pub fn conn() -> rusqlite::Result<Connection> {
    let c = Connection::open(builds_db_path())?;
    c.execute_batch(include_str!("../../migrations/builds.sql"))?;
    Ok(c)
}

/// The editable formula rows the engine consumes (key -> value_text).
pub fn load_formulas() -> rusqlite::Result<BTreeMap<String, String>> {
    let c = conn()?;
    let mut stmt =
        c.prepare("SELECT formula_key, COALESCE(value_text, CAST(value_int AS TEXT), '') \
                   FROM formula_table WHERE dim1='' AND dim2='' AND dim3=''")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// One editable rule the engine consumes. `verification_status` is the honesty flag:
/// anything but WIKI_CONFIRMED/VERIFIED_INGAME is a value nobody has measured yet.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FormulaRow {
    pub formula_key: String,
    pub value: String,
    pub description: Option<String>,
    pub verification_status: String,
    pub source: Option<String>,
    pub is_user_edited: bool,
}

pub fn list_formulas() -> rusqlite::Result<Vec<FormulaRow>> {
    let c = conn()?;
    let mut stmt = c.prepare(
        "SELECT formula_key, COALESCE(value_text, CAST(value_int AS TEXT), ''), \
                description, verification_status, source, is_user_edited \
         FROM formula_table WHERE dim1='' AND dim2='' AND dim3='' \
         ORDER BY verification_status, formula_key",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(FormulaRow {
            formula_key: r.get(0)?, value: r.get(1)?, description: r.get(2)?,
            verification_status: r.get(3)?, source: r.get(4)?,
            is_user_edited: r.get::<_, i64>(5)? != 0,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Edit a formula. Marks it user-edited and, when the user says they measured it in
/// game, promotes the verification status — that is how an unverified number graduates.
pub fn set_formula(key: &str, value: &str, verified_ingame: bool) -> rusqlite::Result<()> {
    let c = conn()?;
    // new formula_version row = an audit trail of every edit batch (plan §2.2)
    c.execute(
        "INSERT INTO formula_version(label, notes) VALUES(?1, 'user edit')",
        [format!("edit {key} @ {}", chrono_now())],
    )
    .ok();
    let status = if verified_ingame { "VERIFIED_INGAME" } else { "MANUAL_OVERRIDE" };
    c.execute(
        "UPDATE formula_table SET value_text=?1, value_int=NULL, is_user_edited=1, \
         verification_status=?2 WHERE formula_key=?3 AND dim1='' AND dim2='' AND dim3=''",
        rusqlite::params![value, status, key],
    )?;
    Ok(())
}

fn chrono_now() -> String {
    // no chrono dep: SQLite gives us the timestamp
    conn()
        .and_then(|c| c.query_row("SELECT datetime('now')", [], |r| r.get::<_, String>(0)))
        .unwrap_or_default()
}

#[derive(serde::Serialize)]
pub struct BuildSummary {
    pub id: i64,
    pub name: String,
    pub level: Option<i64>,
    pub classes: Vec<String>,
    pub updated_at: String,
}

pub fn list_builds() -> rusqlite::Result<Vec<BuildSummary>> {
    let c = conn()?;
    let mut stmt = c.prepare("SELECT id, name, level, updated_at FROM build ORDER BY updated_at DESC")?;
    let mut out: Vec<BuildSummary> = stmt
        .query_map([], |r| {
            Ok(BuildSummary {
                id: r.get(0)?, name: r.get(1)?, level: r.get(2)?,
                classes: Vec::new(), updated_at: r.get(3)?,
            })
        })?
        .filter_map(|x| x.ok())
        .collect();
    let mut cstmt = c.prepare("SELECT class FROM build_class WHERE build_id=?1 ORDER BY slot")?;
    for b in out.iter_mut() {
        b.classes = cstmt
            .query_map([b.id], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
    }
    Ok(out)
}

/// Upsert a build (by name). Equipment saved with soft refs (pageid + canonical name).
pub fn save_build(input: &BuildInput) -> rusqlite::Result<i64> {
    let c = conn()?;
    let existing: Option<i64> = c
        .query_row("SELECT id FROM build WHERE name=?1", [&input.name], |r| r.get(0))
        .ok();
    let id = match existing {
        Some(id) => {
            c.execute(
                "UPDATE build SET race=?1, level=?2, updated_at=datetime('now') WHERE id=?3",
                rusqlite::params![input.race, input.level, id],
            )?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO build(name, race, level, data_version_id) VALUES(?1,?2,?3,1)",
                rusqlite::params![input.name, input.race, input.level],
            )?;
            c.last_insert_rowid()
        }
    };
    c.execute("DELETE FROM build_class WHERE build_id=?1", [id])?;
    for (i, cl) in input.classes.iter().take(3).enumerate() {
        c.execute(
            "INSERT INTO build_class(build_id, slot, class) VALUES(?1,?2,?3)",
            rusqlite::params![id, (i + 1) as i64, cl.to_uppercase()],
        )?;
    }
    c.execute("DELETE FROM build_equipment WHERE build_id=?1", [id])?;
    let snap = crate::db::snapshot();
    // player paperdoll + the pet paperdoll share the table; pet slots are "PET_<SLOT>"
    for (slot, pageid) in input.equipment.iter().chain(input.pet_equipment.iter()) {
        let name = snap
            .items_by_id
            .get(pageid)
            .map(|i| i.name.to_lowercase())
            .unwrap_or_default();
        let tier = input.equipment_tiers.get(slot).copied().unwrap_or(0).min(10);
        c.execute(
            "INSERT INTO build_equipment(build_id, slot, item_pageid, item_name_canonical, \
             upgrade_tier) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![id, slot, pageid, name, tier],
        )?;
    }
    c.execute("DELETE FROM build_spell_tier WHERE build_id=?1", [id])?;
    for (spell_id, tier) in &input.spell_tiers {
        if *tier == 0 {
            continue;
        }
        // the PK is (build_id, spell_name_canonical): spells missing from THIS mirror
        // (e.g. a shared build from a newer wiki) must not all collide on '' — key them
        // uniquely by pageid so every tier survives the round-trip
        let name = snap
            .spell_names
            .get(spell_id)
            .map(|n| n.to_lowercase())
            .unwrap_or_else(|| format!("pageid:{spell_id}"));
        c.execute(
            "INSERT OR REPLACE INTO build_spell_tier(build_id, spell_pageid, \
             spell_name_canonical, spell_upgrade_tier) VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, spell_id, name, (*tier).min(10)],
        )?;
    }
    // pet + bard flags ride in app_meta-style columns we don't have yet: store as wishlist-free
    // JSON blob in build.deity (unused) is ugly — use a dedicated table instead.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS build_extra (
           build_id INTEGER PRIMARY KEY REFERENCES build(id) ON DELETE CASCADE,
           pet_summon_spell_id INTEGER, pet_summon_tier INTEGER NOT NULL DEFAULT 0,
           bard_in_group INTEGER NOT NULL DEFAULT 0)",
    )?;
    // columns added after the table first shipped: guard the ALTERs
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN enabled_eras TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN spellbook TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN loadouts TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN disabled_buffs TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN strict_buffs INTEGER", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN aa_mnemonic INTEGER", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN aa_ranks TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN aa_points INTEGER", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN pet_slot_override INTEGER", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN augments TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN stance TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN invocation TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN external_buffs TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN manual_buffs TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN other_buffs TEXT", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN allow_over_cap INTEGER", []);
    let _ = c.execute("ALTER TABLE build_extra ADD COLUMN disabled_lines TEXT", []);
    c.execute(
        "INSERT OR REPLACE INTO build_extra(build_id, pet_summon_spell_id, pet_summon_tier, \
         bard_in_group, enabled_eras, spellbook, loadouts, disabled_buffs, strict_buffs, \
         aa_mnemonic, aa_ranks, aa_points, pet_slot_override, augments, stance, invocation, \
         external_buffs, manual_buffs, other_buffs, allow_over_cap, disabled_lines) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
        rusqlite::params![
            id,
            input.pet_summon_spell_id,
            input.pet_summon_tier,
            input.bard_in_group as i64,
            serde_json::to_string(&input.enabled_eras).unwrap_or_default(),
            serde_json::to_string(&input.spellbook).unwrap_or_default(),
            serde_json::to_string(&input.loadouts).unwrap_or_default(),
            serde_json::to_string(&input.disabled_buffs).unwrap_or_default(),
            input.strict_buffs as i64,
            input.aa_mnemonic_retention,
            serde_json::to_string(&input.aa_ranks).unwrap_or_default(),
            input.aa_points_available,
            input.pet_slot_override,
            serde_json::to_string(&input.augments).unwrap_or_default(),
            input.stance,
            input.invocation,
            serde_json::to_string(&input.external_buffs).unwrap_or_default(),
            serde_json::to_string(&input.manual_buffs).unwrap_or_default(),
            serde_json::to_string(&input.other_buffs).unwrap_or_default(),
            input.allow_over_cap as i64,
            serde_json::to_string(&input.disabled_lines).unwrap_or_default()
        ],
    )?;
    Ok(id)
}

/// Load a build; soft refs re-resolve by pageid, falling back to canonical name
/// (plan §2.2.0 — renamed/re-synced items reconcile instead of dangling).
pub fn load_build(id: i64) -> rusqlite::Result<BuildInput> {
    let c = conn()?;
    let (name, race, level): (String, Option<String>, Option<i64>) = c.query_row(
        "SELECT name, race, level FROM build WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut input = BuildInput {
        name,
        race,
        level: level.unwrap_or(60) as u32,
        ..Default::default()
    };
    let mut cstmt = c.prepare("SELECT class FROM build_class WHERE build_id=?1 ORDER BY slot")?;
    input.classes = cstmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .filter_map(|x| x.ok())
        .collect();
    let snap = crate::db::snapshot();
    let by_name: BTreeMap<String, i64> = snap
        .items_by_id
        .values()
        .map(|i| (i.name.to_lowercase(), i.pageid))
        .collect();
    let mut estmt = c.prepare(
        "SELECT slot, item_pageid, item_name_canonical, upgrade_tier \
         FROM build_equipment WHERE build_id=?1",
    )?;
    for row in estmt.query_map([id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?,
            r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
    })? {
        let (slot, pid, nm, tier) = row?;
        let resolved = pid
            .filter(|p| snap.items_by_id.contains_key(p))
            .or_else(|| nm.and_then(|n| by_name.get(&n).copied()));
        if let Some(p) = resolved {
            if tier > 0 {
                input.equipment_tiers.insert(slot.clone(), tier as u32);
            }
            if slot.starts_with("PET_") {
                input.pet_equipment.insert(slot, p);
            } else {
                input.equipment.insert(slot, p);
            }
        }
    }
    let mut ststmt = c.prepare(
        "SELECT spell_pageid, spell_upgrade_tier FROM build_spell_tier WHERE build_id=?1",
    )?;
    for row in ststmt.query_map([id], |r| {
        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (sid, tier) = row?;
        if let Some(sid) = sid {
            input.spell_tiers.insert(sid, tier as u32);
        }
    }
    for col in ["enabled_eras TEXT", "spellbook TEXT", "loadouts TEXT",
                "disabled_buffs TEXT", "strict_buffs INTEGER", "aa_mnemonic INTEGER",
                "aa_ranks TEXT", "aa_points INTEGER", "pet_slot_override INTEGER",
                "augments TEXT", "stance TEXT", "invocation TEXT", "external_buffs TEXT",
                "manual_buffs TEXT", "other_buffs TEXT", "allow_over_cap INTEGER",
                "disabled_lines TEXT"] {
        let _ = c.execute(&format!("ALTER TABLE build_extra ADD COLUMN {col}"), []);
    }
    if let Ok((mnem, ranks, pts)) = c.query_row(
        "SELECT COALESCE(aa_mnemonic,0), aa_ranks, COALESCE(aa_points,0) \
         FROM build_extra WHERE build_id=?1",
        [id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)),
    ) {
        input.aa_mnemonic_retention = mnem.clamp(0, 6) as u32;
        input.aa_ranks = ranks
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        input.aa_points_available = pts.max(0) as u32;
    }
    if let Ok((sid, tier, bard, eras, book, los, disb, strict, pet_slots, augs, stance, invocation, ext, man, oth, overcap, dis_lines)) = c.query_row(
        "SELECT pet_summon_spell_id, pet_summon_tier, bard_in_group, enabled_eras, \
         spellbook, loadouts, disabled_buffs, strict_buffs, pet_slot_override, augments, \
         stance, invocation, external_buffs, manual_buffs, other_buffs, allow_over_cap, \
         disabled_lines \
         FROM build_extra WHERE build_id=?1",
        [id],
        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?, r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<i64>>(7)?, r.get::<_, Option<i64>>(8)?,
                r.get::<_, Option<String>>(9)?, r.get::<_, Option<String>>(10)?,
                r.get::<_, Option<String>>(11)?, r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<String>>(13)?, r.get::<_, Option<String>>(14)?,
                r.get::<_, Option<i64>>(15)?, r.get::<_, Option<String>>(16)?)),
    ) {
        fn parse<T: serde::de::DeserializeOwned + Default>(o: Option<String>) -> T {
            o.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
        }
        input.pet_summon_spell_id = sid;
        input.pet_summon_tier = tier as u32;
        input.bard_in_group = bard != 0;
        input.enabled_eras = parse(eras);
        input.spellbook = parse(book);
        input.loadouts = parse(los);
        input.disabled_buffs = parse(disb);
        input.strict_buffs = strict.unwrap_or(0) != 0;
        input.pet_slot_override = pet_slots.filter(|&n| n >= 1).map(|n| n as u32);
        input.augments = parse(augs);
        input.stance = stance.filter(|s| !s.is_empty());
        input.invocation = invocation.filter(|s| !s.is_empty());
        input.external_buffs = parse(ext);
        input.manual_buffs = parse(man);
        input.other_buffs = parse(oth);
        input.allow_over_cap = overcap.unwrap_or(0) != 0;
        input.disabled_lines = parse(dis_lines);
    }
    // pre-paperdoll saves keyed pet gear "PET_1".."PET_N" — re-home each item (with its
    // tier + augments) onto the pet paperdoll by natural wear slot; persisted next save
    eql_data::migrate_legacy_pet_keys(
        &mut input.pet_equipment,
        &mut input.equipment_tiers,
        &mut input.augments,
        |pid| snap.items_by_id.get(&pid).map(|i| i.slots.clone()),
    );
    Ok(input)
}

pub fn delete_build(id: i64) -> rusqlite::Result<()> {
    let c = conn()?;
    c.execute("DELETE FROM build WHERE id=?1", [id])?;
    Ok(())
}
