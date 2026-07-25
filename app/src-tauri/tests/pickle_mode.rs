//! Regression test for the non-canonical ("three Magicians") mode.
//! Guards two things: the reveal actually reveals, and a normal build never sees
//! non-canonical entries -- including through the optimizer, which reads the full
//! snapshot rather than the filtered picker list.
use eql_builder_lib::db;
use eql_data::BuildInput;
use eql_engine::{optimizer::Profile, optimize_gear, resolve_build};
use std::path::PathBuf;
use std::time::Instant;

fn point_at_repo_db() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("..").join("db").join("eql.db");
    assert!(p.exists(), "expected wiki db at {p:?}");
    std::env::set_var("EQL_WIKI_DB", &p);
    let tmp = std::env::temp_dir().join("eql_test_builds_pickle.db");
    let _ = std::fs::remove_file(&tmp);
    std::env::set_var("EQL_BUILDS_DB", tmp);
}

fn build_with(classes: &[&str]) -> BuildInput {
    BuildInput {
        name: "diag".into(),
        level: 60,
        classes: classes.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn pickle_mode_reveals_and_normal_builds_do_not() {
    point_at_repo_db();

    // ---- 1. the picker query, both ways
    let t = Instant::now();
    let normal = db::load_items(&["MAG".to_string()]).expect("load_items MAG");
    println!("load_items[MAG]            -> {} items  ({:?})", normal.len(), t.elapsed());

    let t = Instant::now();
    let mag3: Vec<String> = vec!["MAG".into(), "MAG".into(), "MAG".into()];
    let pickle = db::load_items(&mag3).expect("load_items MAG x3");
    println!("load_items[MAG,MAG,MAG]    -> {} items  ({:?})", pickle.len(), t.elapsed());

    assert!(!normal.is_empty(), "a normal MAG build must see items");
    assert!(!pickle.is_empty(), "pickle mode must still see the normal items");
    assert!(
        normal.iter().all(|i| !i.non_canonical),
        "a normal build must never be shown non-canonical entries"
    );
    let revealed = pickle.iter().filter(|i| i.non_canonical).count();
    println!("non-canonical revealed     -> {revealed}");
    assert!(revealed > 0, "pickle mode must reveal the non-canonical entries");

    // ---- 2. resolve, both ways
    let snap = db::snapshot();
    let t = Instant::now();
    let r1 = resolve_build(&snap, &build_with(&["MAG"]));
    println!("resolve[MAG]               -> ok ({:?})  {} stats", t.elapsed(), r1.stats.len());
    let t = Instant::now();
    let r3 = resolve_build(&snap, &build_with(&["MAG", "MAG", "MAG"]));
    println!("resolve[MAG,MAG,MAG]       -> ok ({:?})  {} stats", t.elapsed(), r3.stats.len());

    // ---- 3. the optimizer, both ways (this is the reported hang)
    let t = Instant::now();
    let o1 = optimize_gear(&snap, &build_with(&["MAG"]), Profile::Optimal, false);
    println!("optimize[MAG]              -> {} slots ({:?})", o1.equipment.len(), t.elapsed());

    let t = Instant::now();
    let o3 = optimize_gear(&snap, &build_with(&["MAG", "MAG", "MAG"]), Profile::Optimal, false);
    println!("optimize[MAG,MAG,MAG]      -> {} slots ({:?})", o3.equipment.len(), t.elapsed());

    // ---- 4. the leak check: a NORMAL optimize must not equip non-canonical gear
    let leaked: Vec<_> = o1
        .equipment
        .values()
        .filter_map(|pid| snap.items_by_id.get(pid))
        .filter(|it| it.non_canonical)
        .map(|it| it.name.clone())
        .collect();
    println!("non-canonical in normal optimize -> {leaked:?}");
    assert!(
        leaked.is_empty(),
        "the optimizer leaked non-canonical gear into an ordinary build: {leaked:?}"
    );

    // ---- 5. ...but the revealed build may absolutely wear them
    let equipped: Vec<_> = o3
        .equipment
        .values()
        .filter_map(|pid| snap.items_by_id.get(pid))
        .filter(|it| it.non_canonical)
        .map(|it| it.name.clone())
        .collect();
    println!("non-canonical in revealed optimize -> {} pieces", equipped.len());
    assert!(
        !equipped.is_empty(),
        "the revealed build should be able to equip the non-canonical set"
    );
}
