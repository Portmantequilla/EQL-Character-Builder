//! INI round-trip against the real game file (spells_us.txt id<->name) + a sample loadout.
use eql_builder_lib::{db, spellbook};
use eql_data::SpellLoadout;
use std::path::PathBuf;

fn setup() {
    let db_p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("db").join("eql.db");
    std::env::set_var("EQL_WIKI_DB", &db_p);
    std::env::set_var("EQL_GAME_DIR", "E:/EQL");
    std::env::set_var("EQL_BUILDS_DB", std::env::temp_dir().join("eql_ini_builds.db"));
}

#[test]
fn ini_round_trips_through_game_names() {
    setup();
    let snap = db::snapshot();
    // pick a few known spells by wiki name that also exist in spells_us.txt
    let want = ["Spirit of Wolf", "Health", "Resist Fire"];
    let picks: Vec<Option<i64>> = want.iter().map(|n|
        snap.spell_names.iter().find(|(_, name)| name.as_str() == *n).map(|(id, _)| *id)
    ).collect();
    let scribed = picks.iter().filter(|p| p.is_some()).count();
    let lo = SpellLoadout { name: "Test".into(), slots: picks.clone() };
    let ini = spellbook::export_ini(&[lo], &snap.spell_names);
    assert!(ini.contains("[SpellLoadouts]"));
    // at least the resolvable ones become real game ids (not -1)
    let realized = ini.lines().filter(|l| l.contains(".slot") && !l.ends_with("=-1")).count();
    assert!(realized >= scribed.saturating_sub(1),
        "expected ~{scribed} game ids, got {realized}:\n{ini}");

    // write + re-import
    let tmp = std::env::temp_dir().join("eql_test_loadout.ini");
    std::fs::write(&tmp, &ini).unwrap();
    let back = spellbook::import_ini(&tmp, &snap.spell_names).unwrap();
    assert_eq!(back.len(), 1);
    // Spirit of Wolf should survive the wiki->game->wiki round-trip
    if let Some(sow) = picks[0] {
        assert!(back[0].slots.iter().flatten().any(|&w| w == sow),
            "Spirit of Wolf lost in round-trip: {:?}", back[0].slots);
    }
}
