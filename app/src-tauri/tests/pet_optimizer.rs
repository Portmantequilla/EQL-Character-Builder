//! Integration check for the pet gear suggester against the real wiki snapshot: a summoned
//! pet gets survival gear filling only its slot budget, and the player's own gear is untouched.
use eql_builder_lib::db;
use eql_data::BuildInput;
use eql_engine::{optimize_pet_gear, Profile};

#[test]
fn pet_optimizer_fills_budget_with_pet_legal_gear() {
    let snap = db::snapshot();
    if snap.items_by_id.is_empty() || snap.pet_summons.is_empty() {
        return; // no wiki DB in this environment — skip rather than false-fail
    }

    // pick any summon a class can cast by level 50, and an owner class that casts it
    let Some((&sid, owner_class)) = snap.pet_summons.iter().find_map(|(id, s)| {
        s.class_levels
            .iter()
            .find(|(_, &lv)| lv <= 50)
            .map(|(c, _)| (id, c.clone()))
    }) else {
        return;
    };

    let mut b = BuildInput {
        level: 50,
        classes: vec![owner_class],
        race: Some("Human".into()),
        pet_summon_spell_id: Some(sid),
        pet_slot_override: Some(6), // force a known budget, independent of class-combo bonuses
        ..Default::default()
    };
    // a stale player pick + pet pick that the optimizer must handle correctly
    b.equipment.insert("CHEST".into(), 34940);
    b.pet_equipment.insert("PET_HEAD".into(), 999_999);

    let out = optimize_pet_gear(&snap, &b, Profile::Optimal, false);

    // player gear preserved untouched
    assert_eq!(out.equipment.get("CHEST"), Some(&34940), "player gear must not change");

    let pet: Vec<(&String, &i64)> = out.pet_equipment.iter().collect();
    // filled at most the budget, and every key is a PET_ slot
    assert!(pet.len() <= 6, "filled {} > budget 6", pet.len());
    assert!(pet.iter().all(|(k, _)| k.starts_with("PET_")), "only pet slots filled");
    // no duplicate items across pet slots
    let mut pids: Vec<i64> = pet.iter().map(|(_, &v)| v).collect();
    pids.sort_unstable();
    let uniq = { let mut u = pids.clone(); u.dedup(); u.len() };
    assert_eq!(uniq, pids.len(), "no duplicate pageids across pet slots");
    // the stale 999_999 placeholder was cleared (it isn't a real item)
    assert!(!pids.contains(&999_999), "stale pet pick replaced");

    // the DB has ample pet-wearable survival gear, so a 6-slot budget should actually fill
    assert!(!pet.is_empty(), "expected the optimizer to gear the pet");

    // every chosen item is legal for the PET's class pool (its own classes, not the owner's)
    let pool = eql_engine::pet::pet_class_pool(&snap, &b);
    let pool_up: Vec<String> = pool.iter().map(|c| c.to_uppercase()).collect();
    for (_slot, pid) in &pet {
        let it = snap.items_by_id.get(pid).expect("chosen item exists");
        let ok = it.classes.iter().any(|c| c == "ALL")
            || it.classes.iter().any(|ic| pool_up.iter().any(|c| c == ic));
        assert!(ok, "item {} ({}) not legal for pet pool {:?}", pid, it.name, pool);
    }
}
