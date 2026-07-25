//! End-to-end check of the inventory importer against the REAL wiki DB and a REAL
//! `/outputfile inventory` dump. Both paths come from env vars and the test skips when
//! either is missing, so CI (or any other machine) just passes over it. Run with:
//!   EQL_WIKI_DB=<path>/eql.db EQL_TEST_INVENTORY=<path>/<Char>_<city>-Inventory.txt \
//!     cargo test -p eql-builder --test inventory_realfile -- --nocapture
use std::path::Path;

#[test]
fn real_inventory_file_resolves_like_the_python_crosscheck() {
    let db = std::env::var("EQL_WIKI_DB").unwrap_or_default();
    let file = std::env::var("EQL_TEST_INVENTORY").unwrap_or_default();
    if db.is_empty() || file.is_empty() || !Path::new(&db).exists() || !Path::new(&file).exists() {
        eprintln!("skipping real-file test (set EQL_WIKI_DB and EQL_TEST_INVENTORY)");
        return;
    }
    let file = file.as_str();
    let items = eql_builder_lib::db::snapshot().items_by_id.clone();
    let imp = eql_builder_lib::inventory::import_file(Path::new(file), &items).unwrap();
    println!(
        "character={:?} matched={} unmatched={} exaltations={}",
        imp.character, imp.matched.len(), imp.unmatched.len(), imp.exaltations.len()
    );
    for m in &imp.matched {
        println!("  MATCH {:<10} +{:<2} {}", m.slot, m.tier, m.game_name);
    }
    for u in &imp.unmatched {
        println!("  MISS  {:<10}    {} ({})", u.slot, u.game_name, u.reason);
    }
    // the Python cross-check found 19/20 worn items, with only "Fippy's Paw" absent
    assert!(imp.character.is_some_and(|c| !c.is_empty()), "character name parsed from the filename");
    assert!(imp.matched.len() >= 19, "expected >=19 matched, got {}", imp.matched.len());
    assert!(
        imp.unmatched.iter().any(|u| u.base_name.contains("Fippy")),
        "expected the undocumented Fippy's Paw among unmatched"
    );
    // a couple of specific tiers we know from the dump
    assert_eq!(imp.equipment_tiers.get("SECONDARY"), Some(&10)); // Morning Star +10
    assert!(imp.exaltations.len() >= 7, "expected the 7 Exaltation sub-rows");
    // all 7 sockets map to types + resolve their sources (7/7 verified 2026-07-15):
    // socket7=FOCUS x5, socket8=CLICK (Refugee Shroud), socket10=PROC (Ruby Stiletto)
    let n_augs: usize = imp.augments.values().map(|m| m.len()).sum();
    assert_eq!(n_augs, 7, "all 7 Exaltations resolve into build-ready augments");
    assert!(imp.augments.get("PRIMARY").is_some_and(|m| m.contains_key("PROC")));
    assert!(imp.augments.get("SHOULDERS").is_some_and(|m| m.contains_key("CLICK")));
}
