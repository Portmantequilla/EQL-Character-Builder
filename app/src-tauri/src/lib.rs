//! Tauri shell library: SQLite loading + command handlers. The engine stays pure —
//! this layer feeds it snapshots (plan §4.1).
pub mod builds;
pub mod db;
pub mod inventory;
pub mod lootfilter;
pub mod menu;
pub mod socials;
pub mod spellbook;

use eql_data::{BuildCalculationResult, BuildInput, Item};
use eql_engine::{choose_for_me as engine_choose, resolve_build as engine_resolve};

/// Items wearable by the given classes (empty = all): the gear browser.
#[tauri::command]
fn query_items(classes: Vec<String>) -> Result<Vec<Item>, String> {
    db::load_items(&classes).map_err(|e| e.to_string())
}

/// The one result object every page renders (plan §4.2/§12).
#[tauri::command]
fn resolve_build(build: BuildInput) -> Result<BuildCalculationResult, String> {
    Ok(engine_resolve(&db::snapshot(), &build))
}

/// Spells page: everything any of the build's classes gets at or below level.
#[tauri::command]
fn query_spells(classes: Vec<String>, level: u32) -> Result<Vec<db::SpellRow>, String> {
    db::query_spells(&classes, level).map_err(|e| e.to_string())
}

/// Icon id for a set of spell pageids (spellbook squares show the spell gem icon).
#[tauri::command]
fn spell_icons(ids: Vec<i64>) -> Result<std::collections::BTreeMap<i64, String>, String> {
    db::spell_icons(&ids).map_err(|e| e.to_string())
}

/// The full AA table (planner). Availability is decided in the UI/engine, not here.
#[tauri::command]
fn list_aas() -> Result<Vec<eql_data::AaAbility>, String> {
    Ok(db::snapshot().aas.clone())
}

/// The augment catalog: every effect-bearing item as the "<item> (Exaltation)" augment
/// it can become at +4 (regen effects excluded). Feeds the item-edit popup dropdowns.
#[tauri::command]
fn list_augments() -> Result<Vec<eql_data::AugmentInfo>, String> {
    Ok(eql_engine::augments::augment_catalog(&db::snapshot()))
}

/// What linked effect spells do (hover explanations for item/augment effects).
#[tauri::command]
fn spell_details(
    ids: Vec<i64>,
) -> Result<std::collections::BTreeMap<i64, db::SpellDetails>, String> {
    db::spell_details(&ids).map_err(|e| e.to_string())
}

/// Combat modes — stances + invocations (eqlbuilds snapshot; display-only v1).
#[tauri::command]
fn list_modes() -> Result<Vec<db::Mode>, String> {
    db::list_modes().map_err(|e| e.to_string())
}

/// Skill lines for the build's classes, merged BEST_OF (highest cap wins).
#[tauri::command]
fn query_skills(classes: Vec<String>) -> Result<Vec<db::SkillRow>, String> {
    db::query_skills(&classes).map_err(|e| e.to_string())
}

/// Spell ids receivable as external buffs (non-self-only buff-line members).
#[tauri::command]
fn external_receivable() -> Result<Vec<i64>, String> {
    db::external_receivable().map_err(|e| e.to_string())
}

/// The `?` popup: one spell's mechanics, stacking lines, and acquisition rows.
#[tauri::command]
fn spell_info(id: i64) -> Result<db::SpellInfo, String> {
    db::spell_info(id).map_err(|e| e.to_string())
}

/// The "Add Other Buff" catalog: clicky/worn/proc/consumable buff members.
#[tauri::command]
fn list_other_buffs() -> Result<Vec<db::OtherBuffRow>, String> {
    db::list_other_buffs().map_err(|e| e.to_string())
}

/// The Focus Effects reference tab: every FOCUS-bearing item + drop sources.
#[tauri::command]
fn focus_effects() -> Result<Vec<db::FocusEffectRow>, String> {
    db::focus_effects().map_err(|e| e.to_string())
}

/// The Exaltations reference tab: every FOCUS/CLICK/WORN/PROC-bearing item (regen
/// excluded — not extractable) + drop sources, grouped client-side by kind.
#[tauri::command]
fn exaltation_effects() -> Result<Vec<db::ExaltationRow>, String> {
    db::exaltation_effects().map_err(|e| e.to_string())
}

/// Structured effect rows for the character's worn FOCUS spells — the Spellbook
/// applies their percentages to displayed mana/cast/damage/healing.
#[tauri::command]
fn focus_details(ids: Vec<i64>) -> Result<Vec<db::FocusDetailRow>, String> {
    db::focus_details(&ids).map_err(|e| e.to_string())
}

/// Structured focus effects decoded from the client (exact limits) for the Spellbook.
#[tauri::command]
fn focus_client(ids: Vec<i64>) -> Result<Vec<db::FocusClient>, String> {
    db::focus_client(&ids).map_err(|e| e.to_string())
}

/// Open a URL in the system browser (the `?` popup's wiki link).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https links".into());
    }
    std::process::Command::new("explorer")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// spell pageid -> buff line name (spellbook auto-organize "by line" grouping).
#[tauri::command]
fn spell_lines() -> Result<std::collections::BTreeMap<i64, String>, String> {
    db::spell_line_map().map_err(|e| e.to_string())
}

/// Farm list: dropping mobs + zones for the named items.
#[tauri::command]
fn farm_list(item_names: Vec<String>) -> Result<Vec<db::FarmSource>, String> {
    db::farm_sources(&item_names).map_err(|e| e.to_string())
}

/// Seeded random build (plan "Choose for me"). Same seed -> same build.
#[tauri::command]
fn choose_for_me(
    seed: u64,
    level: u32,
    classes: Vec<String>,
    enabled_eras: Vec<String>,
) -> Result<BuildInput, String> {
    Ok(engine_choose(&db::snapshot(), seed, level, classes, enabled_eras))
}

/// One-click gear optimization: profile "OPTIMAL" (survival/longevity) or "MINMAX"
/// (max offense). Returns the build with its worn player gear + Exaltations replaced.
#[tauri::command]
fn optimize_gear(
    build: BuildInput,
    profile: String,
    allow_epic: Option<bool>,
) -> Result<BuildInput, String> {
    let p = eql_engine::Profile::parse(&profile)
        .ok_or_else(|| format!("unknown optimize profile: {profile}"))?;
    Ok(eql_engine::optimize_gear(&db::snapshot(), &build, p, allow_epic.unwrap_or(false)))
}

/// Suggest gear for the PET (survival by default), filling only its active-slot budget with
/// the best items the pet's class pool can wear. Leaves player gear + the pet summon intact.
#[tauri::command]
fn optimize_pet_gear(
    build: BuildInput,
    profile: String,
    allow_epic: Option<bool>,
) -> Result<BuildInput, String> {
    let p = eql_engine::Profile::parse(&profile)
        .ok_or_else(|| format!("unknown optimize profile: {profile}"))?;
    Ok(eql_engine::optimize_pet_gear(&db::snapshot(), &build, p, allow_epic.unwrap_or(false)))
}

#[tauri::command]
fn get_static() -> Result<db::StaticData, String> {
    Ok(db::static_data())
}

#[tauri::command]
fn save_build(build: BuildInput) -> Result<i64, String> {
    builds::save_build(&build).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_builds() -> Result<Vec<builds::BuildSummary>, String> {
    builds::list_builds().map_err(|e| e.to_string())
}

#[tauri::command]
fn load_build(id: i64) -> Result<BuildInput, String> {
    builds::load_build(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_build(id: i64) -> Result<(), String> {
    builds::delete_build(id).map_err(|e| e.to_string())
}

/// Spellbook loadouts -> paste-ready [SpellLoadouts] .ini on the Desktop.
/// Reveals the file in Explorer so the export can never land somewhere invisible.
#[tauri::command]
fn export_spellbook(
    build_name: String,
    loadouts: Vec<eql_data::SpellLoadout>,
) -> Result<String, String> {
    let path = spellbook::export_to_desktop(&build_name, &loadouts, &db::snapshot().spell_names)
        .map_err(|e| e.to_string())?;
    spellbook::reveal_in_explorer(&path);
    Ok(path)
}

/// Import loadouts from a game/loadout .ini (game spell ids -> wiki pageids by name).
#[tauri::command]
fn import_spellbook(path: String) -> Result<Vec<eql_data::SpellLoadout>, String> {
    spellbook::import_ini(std::path::Path::new(&path), &db::snapshot().spell_names)
        .map_err(|e| e.to_string())
}

/// The real `<Char>_<city>_LO1.ini` settings files in the EQL folder we can safely merge into.
#[tauri::command]
fn list_loadout_files() -> Vec<spellbook::LoadoutFile> {
    spellbook::list_loadout_files()
}

/// Merge the build's spell sets INTO a live `<Char>_<city>_LO1.ini`, preserving every other
/// section (hotbars, socials, sound…) and every set the build doesn't define. Backs the file
/// up to `<name>.bak` first. This is the integrity-safe alternative to the Desktop fragment.
#[tauri::command]
fn export_spellbook_to_game(
    path: String,
    loadouts: Vec<eql_data::SpellLoadout>,
) -> Result<spellbook::LoadoutWrite, String> {
    spellbook::write_loadouts_to_char_ini(
        std::path::Path::new(&path),
        &loadouts,
        &db::snapshot().spell_names,
    )
    .map_err(|e| e.to_string())
}

// -------------------------------------------------------------------- Macros (socials) tab

/// Read the social macros ([Socials]) out of a `<Char>_<city>_LO1.ini` (or any INI with them).
#[tauri::command]
fn read_socials(path: String) -> Result<Vec<socials::Social>, String> {
    socials::read_socials_file(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Replace the `[Socials]` section of a live LO1 file with these macros, preserving every other
/// section (spell loadouts, hotbars, sound…). Backs the file up to `<name>.bak` first.
#[tauri::command]
fn write_socials(path: String, socials: Vec<socials::Social>) -> Result<socials::SocialWrite, String> {
    socials::write_socials_to_char_ini(std::path::Path::new(&path), &socials).map_err(|e| e.to_string())
}

/// Write a shareable `[Socials]` fragment to Desktop/EQLBuilder Exports/<label>_macros.ini.
#[tauri::command]
fn export_socials_desktop(label: String, socials: Vec<socials::Social>) -> Result<String, String> {
    let dir = spellbook::exports_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = label.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let stem = if safe.is_empty() { "macros".to_string() } else { safe };
    let path = dir.join(format!("{stem}_macros.ini"));
    std::fs::write(&path, socials::socials_fragment(&socials)).map_err(|e| e.to_string())?;
    let p = path.display().to_string();
    spellbook::reveal_in_explorer(&p);
    Ok(p)
}

/// Import worn equipment (with +N tiers) from a `/outputfile inventory` dump. Game item
/// names bridge to wiki pageids by name; the file's folder is remembered as the EQL dir
/// so the next import can auto-find dumps without another Browse.
#[tauri::command]
fn import_inventory(path: String) -> Result<inventory::InventoryImport, String> {
    let p = std::path::Path::new(&path);
    let imp = inventory::import_file(p, &db::snapshot().items_by_id).map_err(|e| e.to_string())?;
    if let Some(dir) = p.parent() {
        inventory::remember_eql_dir(dir);
    }
    Ok(imp)
}

/// The `*-Inventory.txt` dumps in the remembered/guessed EQL folder, newest first
/// (the Equipment tab's quick-pick list). Empty when no EQL folder is known yet.
#[tauri::command]
fn list_inventory_files() -> inventory::InventoryScan {
    inventory::list_inventory_files()
}

/// Point the app at the EQL game folder (where `/outputfile inventory` writes) and
/// re-scan it. Lets users whose game lives on a non-default drive set it once.
#[tauri::command]
fn set_eql_dir(path: String) -> inventory::InventoryScan {
    inventory::remember_eql_dir(std::path::Path::new(&path));
    inventory::list_inventory_files()
}

// -------------------------------------------------------------------- Loot Filter tab

/// The `LF_<Char>_<city>.ini` filters in the game's userdata folder, newest first.
#[tauri::command]
fn lf_list_files() -> lootfilter::LfScan {
    lootfilter::list_files()
}

/// Read + parse a loot-filter file for the editor (also harvests its real game ids into
/// the picker catalog).
#[tauri::command]
fn lf_read(path: String) -> Result<lootfilter::LfDoc, String> {
    lootfilter::read_file(std::path::Path::new(&path), &db::snapshot().items_by_id)
        .map_err(|e| e.to_string())
}

/// Write the editor's entries to `LF_<char>_<city>.ini` (backs up any existing file first).
/// Returns the path written.
#[tauri::command]
fn lf_write(character: String, city: String, entries: Vec<lootfilter::LfEntry>) -> Result<String, String> {
    lootfilter::write_file(&character, &city, &entries, None).map_err(|e| e.to_string())
}

/// Harvest an inventory dump's game ids into the picker catalog. Returns rows written.
#[tauri::command]
fn lf_import_inventory(path: String) -> Result<usize, String> {
    let p = std::path::Path::new(&path);
    let n = lootfilter::import_inventory_catalog(p, &db::snapshot().items_by_id)
        .map_err(|e| e.to_string())?;
    if let Some(dir) = p.parent() {
        inventory::remember_eql_dir(dir);
    }
    Ok(n)
}

/// Search the known-game-item catalog (items we have a real game id for) by name.
#[tauri::command]
fn lf_catalog_search(query: String, limit: Option<usize>) -> Result<Vec<lootfilter::CatalogItem>, String> {
    lootfilter::catalog_search(&query, limit.unwrap_or(60)).map_err(|e| e.to_string())
}

/// How many items the catalog knows real game ids for (UI badge).
#[tauri::command]
fn lf_catalog_count() -> Result<i64, String> {
    lootfilter::catalog_count().map_err(|e| e.to_string())
}

/// Search all wiki items by name; each result carries a real game id when the catalog has one.
#[tauri::command]
fn lf_wiki_search(query: String, limit: Option<usize>) -> Vec<lootfilter::WikiPick> {
    lootfilter::wiki_search(&query, limit.unwrap_or(60), &db::snapshot().items_by_id)
}

/// Credits + legal shown in the About dialog. Kept in Rust so the version comes
/// straight from Cargo.toml and can't drift from the packaged build.
#[derive(serde::Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub author: &'static str,
    pub org: &'static str,
    pub copyright: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "EQL Character Builder",
        version: menu::APP_VERSION,
        // name only — no credentials, no data paths (user request 2026-07-19)
        author: "Bee Canyon Retro",
        org: "Bee Canyon Retro",
        copyright: "© 2026 Bee Canyon Retro. All rights reserved."
            .to_string(),
    }
}

// ---------------------------------------------------------------- Settings
/// Every editable rule the engine uses, with its verification status.
#[tauri::command]
fn list_formulas() -> Result<Vec<builds::FormulaRow>, String> {
    builds::list_formulas().map_err(|e| e.to_string())
}

/// Edit a rule. `verified_ingame` = "I measured this in game" -> promotes its status.
/// Refreshes the snapshot so the next resolve uses the new value immediately.
#[tauri::command]
fn set_formula(key: String, value: String, verified_ingame: bool) -> Result<(), String> {
    builds::set_formula(&key, &value, verified_ingame).map_err(|e| e.to_string())?;
    db::refresh_snapshot();
    Ok(())
}

// ---------------------------------------------------------------- Build sharing
/// Write the build to Desktop/EQLBuilder Exports/<name>.eqlbuild.json (shareable).
#[tauri::command]
fn export_build(build: BuildInput) -> Result<String, String> {
    let dir = spellbook::exports_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = build
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!(
        "{}.eqlbuild.json",
        if safe.is_empty() { "build".into() } else { safe }
    ));
    let json = serde_json::to_string_pretty(&build).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    let p = path.display().to_string();
    spellbook::reveal_in_explorer(&p);
    Ok(p)
}

/// Read a shared build file back in.
#[tauri::command]
fn import_build(path: String) -> Result<BuildInput, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("not a valid build file: {e}"))
}

/// Reveal the folder holding the databases (File > Open Data Folder).
#[tauri::command]
fn open_data_folder() -> Result<(), String> {
    let dir = builds::builds_db_path()
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or("no data folder")?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// First run of an installed copy: seed %LOCALAPPDATA%/EQLBuilder/wiki.db from the
/// database bundled with the installer. Dev runs (which find ../db/eql.db) skip this.
fn seed_wiki_db(app: &tauri::AppHandle) {
    use tauri::Manager;
    let target = match std::env::var("LOCALAPPDATA") {
        Ok(l) => std::path::PathBuf::from(l).join("EQLBuilder").join("wiki.db"),
        Err(_) => return,
    };
    if target.exists() {
        return;
    }
    let Ok(bundled) = app.path().resolve("resources/wiki.db", tauri::path::BaseDirectory::Resource)
    else {
        return;
    };
    if !bundled.exists() {
        return; // dev run: db::wiki_db_path() falls back to the repo copy
    }
    if let Some(dir) = target.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::copy(&bundled, &target) {
        Ok(bytes) => eprintln!("seeded wiki.db ({} MB) -> {}", bytes / 1_048_576, target.display()),
        Err(e) => eprintln!("could not seed wiki.db: {e}"),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            seed_wiki_db(&handle);
            let m = menu::build_menu(&handle)?;
            app.set_menu(m)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            menu::on_menu_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            query_items,
            resolve_build,
            query_spells,
            farm_list,
            choose_for_me,
            optimize_gear,
            optimize_pet_gear,
            get_static,
            save_build,
            list_builds,
            load_build,
            delete_build,
            export_spellbook,
            import_spellbook,
            list_loadout_files,
            export_spellbook_to_game,
            read_socials,
            write_socials,
            export_socials_desktop,
            import_inventory,
            list_inventory_files,
            set_eql_dir,
            lf_list_files,
            lf_read,
            lf_write,
            lf_import_inventory,
            lf_catalog_search,
            lf_catalog_count,
            lf_wiki_search,
            spell_icons,
            spell_lines,
            list_aas,
            list_augments,
            spell_details,
            list_modes,
            query_skills,
            external_receivable,
            spell_info,
            list_other_buffs,
            focus_effects,
            exaltation_effects,
            focus_details,
            focus_client,
            open_url,
            app_info,
            open_data_folder,
            list_formulas,
            set_formula,
            export_build,
            import_build
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
