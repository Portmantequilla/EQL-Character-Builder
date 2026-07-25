//! Full-corpus equivalence test: run the Rust resolver over the SAME inputs as the Python
//! oracle (exports/buff_lines.json + oracle-derived class levels) and require identical
//! per-line results (docs/M2-handoff.md acceptance: 70 fillable = 20 self-cast + 50 item).
//! The LIB stays I/O-free; tests may read fixture files.
use eql_data::{BuffLine, MemberStatus, Target};
use eql_engine::{resolve_buff_lines, resolver, SpellClassLevels, SpellNames};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The Python oracle predates the 2026-07-17 sourcing spec: in ITS world every item
/// click/worn/proc auto-applies. Reproduce that world by enabling every member name in
/// `cons.other`, so the corpus equivalence stays a meaningful check of the resolution
/// math (per-line strongest, stacking, values) rather than of the sourcing policy.
fn oracle_world_resolve(
    lines: &[BuffLine],
    clevels: &SpellClassLevels,
    names: &SpellNames,
    build: &[String],
) -> Vec<eql_data::LineResolution> {
    let mut cons = resolver::ResolveConstraints::default();
    for l in lines {
        for m in &l.members {
            if let Some(n) = m.spell_id.and_then(|sid| names.get(&sid)) {
                cons.other.insert(n.to_lowercase());
            }
            if let Some(n) = &m.member_name_raw {
                cons.other.insert(n.to_lowercase());
            }
        }
    }
    resolver::resolve_buff_lines_full(
        lines, clevels, names, build, 60, Target::Player, false,
        &BTreeMap::new(), 10.0, &cons,
    )
    .into_iter()
    .map(|f| f.res)
    .collect()
}

fn base() -> PathBuf {
    // crates/eql-engine -> app -> E:\EQLBuilder
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..")
}

fn load_fixtures() -> (Vec<BuffLine>, SpellClassLevels, SpellNames) {
    let lines: Vec<BuffLine> = serde_json::from_str(
        &std::fs::read_to_string(base().join("exports/buff_lines.json")).unwrap()).unwrap();
    let clevels: BTreeMap<String, BTreeMap<String, u32>> = serde_json::from_str(
        &std::fs::read_to_string(base().join("exports/oracle_class_levels.json")).unwrap()).unwrap();
    let clevels: SpellClassLevels = clevels.into_iter()
        .map(|(k, v)| (k.parse::<i64>().unwrap(), v)).collect();
    let names: BTreeMap<String, String> = serde_json::from_str(
        &std::fs::read_to_string(base().join("exports/oracle_spell_names.json")).unwrap()).unwrap();
    let names: SpellNames = names.into_iter()
        .map(|(k, v)| (k.parse::<i64>().unwrap(), v)).collect();
    (lines, clevels, names)
}

#[test]
fn corpus_matches_python_oracle_shd_mnk_shm_l60() {
    let (lines, clevels, names) = load_fixtures();
    let build = vec!["SHD".to_string(), "MNK".to_string(), "SHM".to_string()];
    let res = oracle_world_resolve(&lines, &clevels, &names, &build);

    // headline acceptance numbers (M2 handoff DoD)
    assert_eq!(res.len(), 112, "player lines");
    let covered: Vec<_> = res.iter().filter(|r| r.chosen.is_some()).collect();
    let self_cast = covered.iter()
        .filter(|r| r.chosen.as_ref().unwrap().status == MemberStatus::SelfCast).count();
    let item = covered.iter()
        .filter(|r| r.chosen.as_ref().unwrap().status == MemberStatus::Item).count();
    assert_eq!(covered.len(), 70, "fillable lines");
    assert_eq!(self_cast, 20, "self-cast lines");
    assert_eq!(item, 50, "item lines");

    // per-line equivalence against the oracle's dumped resolution
    let expected: BTreeMap<String, Option<(String, Option<f64>, String)>> =
        serde_json::from_str(&std::fs::read_to_string(
            base().join("exports/oracle_resolution_shd_mnk_shm_60.json")).unwrap()).unwrap();
    assert_eq!(expected.len(), res.len());
    for r in &res {
        let exp = expected.get(&r.line).unwrap_or_else(|| panic!("line {} missing", r.line));
        match (&r.chosen, exp) {
            (None, None) => {}
            (Some(c), Some((en, ev, es))) => {
                let status = match c.status {
                    MemberStatus::SelfCast => "SELF_CAST",
                    MemberStatus::GroupBard => "GROUP_BARD", // never occurs with bard=false
                    MemberStatus::Item => "ITEM",
                    MemberStatus::External => "EXTERNAL",
                    // never occurs in the oracle corpus (no external_buffs there)
                    MemberStatus::ExternalCast => "EXTERNAL_CAST",
                    MemberStatus::Unknown => "UNKNOWN",
                };
                assert_eq!(&c.name, en, "line {}: name", r.line);
                assert_eq!(c.value, *ev, "line {}: value", r.line);
                assert_eq!(status, es, "line {}: status", r.line);
            }
            (got, exp) => panic!("line {}: engine={:?} oracle={:?}", r.line, got.is_some(), exp.is_some()),
        }
    }
}

#[test]
fn corpus_golden_cases() {
    let (lines, clevels, names) = load_fixtures();
    let get = |build: &[&str], level: u32, line: &str| -> Option<(String, f64, String)> {
        let b: Vec<String> = build.iter().map(|s| s.to_string()).collect();
        let res = resolve_buff_lines(&lines, &clevels, &names, &b, level, Target::Player, false);
        res.iter().find(|r| r.line == line).and_then(|r| r.chosen.as_ref()).map(|c| {
            (c.name.clone(), c.value.unwrap(),
             format!("{:?}", c.status).to_uppercase())
        })
    };
    assert_eq!(get(&["SHD","MNK","SHM"], 60, "Strength (Primary)"),
               Some(("Maniacal Strength".into(), 68.0, "SELFCAST".into())));
    assert_eq!(get(&["SHD","MNK","SHM"], 60, "Strength (Power)"),
               Some(("Focus of Spirit".into(), 67.0, "SELFCAST".into())));
    assert_eq!(get(&["SHD","MNK","SHM"], 60, "Strength (Anthem)"), None);
    assert_eq!(get(&["SHM"], 56, "Strength (Primary)"),
               Some(("Strength".into(), 67.0, "SELFCAST".into())));
    assert_eq!(get(&["SHM"], 57, "Strength (Primary)"),
               Some(("Maniacal Strength".into(), 68.0, "SELFCAST".into())));
}

#[test]
fn corpus_determinism() {
    let (lines, clevels, names) = load_fixtures();
    let build = vec!["SHD".to_string(), "MNK".to_string(), "SHM".to_string()];
    let a = serde_json::to_string(
        &resolve_buff_lines(&lines, &clevels, &names, &build, 60, Target::Player, false)).unwrap();
    let b = serde_json::to_string(
        &resolve_buff_lines(&lines, &clevels, &names, &build, 60, Target::Player, false)).unwrap();
    assert_eq!(a, b);
}
