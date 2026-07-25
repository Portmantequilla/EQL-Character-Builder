//! Spellbook import/export in the game's own format.
//!
//! The client stores memorized-spell sets in a loadout INI ([SpellLoadouts] with
//! SpellLoadoutN.slotM = <game spell id>). The GAME's spell ids differ from our wiki
//! pageids, so we bridge through spell NAME using the client's spells_us.txt
//! (id^name^...). Export writes a paste-ready .ini fragment to the Desktop; import
//! reads either the loadout INI or the whole char INI and returns the sets by wiki id.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use eql_data::SpellLoadout;
use serde::Serialize;

use crate::inventory::guess_eql_dir;

/// Where the client lives (for spells_us.txt). Overridable for testing.
fn eq_dir() -> PathBuf {
    if let Ok(p) = std::env::var("EQL_GAME_DIR") {
        return PathBuf::from(p);
    }
    PathBuf::from("E:/EQL")
}

/// game spell id <-> canonical name, from <eq>/spells_us.txt (id^name^...).
fn game_spell_names() -> BTreeMap<i64, String> {
    let mut out = BTreeMap::new();
    let path = eq_dir().join("spells_us.txt");
    let Ok(text) = std::fs::read_to_string(&path) else { return out };
    for line in text.lines() {
        let mut it = line.splitn(3, '^');
        if let (Some(id), Some(name)) = (it.next(), it.next()) {
            if let Ok(id) = id.parse::<i64>() {
                if !name.is_empty() {
                    out.insert(id, name.to_string());
                }
            }
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.replace('`', "'").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// wiki pageid -> game spell id, joined on normalized name (best-effort).
fn wiki_to_game(spell_names: &BTreeMap<i64, String>) -> BTreeMap<i64, i64> {
    let game = game_spell_names();
    let by_name: BTreeMap<String, i64> = game.iter().map(|(id, n)| (norm(n), *id)).collect();
    spell_names
        .iter()
        .filter_map(|(wiki, name)| by_name.get(&norm(name)).map(|g| (*wiki, *g)))
        .collect()
}

/// Build a paste-ready [SpellLoadouts] INI fragment from the build's loadouts.
/// `spell_names` is the wiki id->name map (from the snapshot).
pub fn export_ini(loadouts: &[SpellLoadout], spell_names: &BTreeMap<i64, String>) -> String {
    let w2g = wiki_to_game(spell_names);
    let mut out = String::from("[SpellLoadouts]\n");
    for (i, lo) in loadouts.iter().enumerate() {
        let n = i + 1;
        out.push_str(&format!("SpellLoadout{n}.inuse=1\n"));
        if !lo.name.is_empty() {
            out.push_str(&format!("SpellLoadout{n}.name={}\n", lo.name));
        }
        // the game always writes MAX_SPELL_GEMS slots per set; -1 = empty gem
        for si in 0..eql_data::MAX_SPELL_GEMS {
            let game_id = lo
                .slots
                .get(si)
                .copied()
                .flatten()
                .and_then(|wiki| w2g.get(&wiki).copied())
                .unwrap_or(-1);
            out.push_str(&format!("SpellLoadout{n}.slot{}={game_id}\n", si + 1));
        }
    }
    out
}

/// Write the export to <Desktop>/EQLBuilder Exports/<build>_spellbook.ini and return the path.
pub fn export_to_desktop(
    build_name: &str,
    loadouts: &[SpellLoadout],
    spell_names: &BTreeMap<i64, String>,
) -> std::io::Result<String> {
    let desktop = dirs_desktop();
    let dir = desktop.join("EQLBuilder Exports");
    std::fs::create_dir_all(&dir)?;
    let safe: String = build_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{}_spellbook.ini", if safe.is_empty() { "build".into() } else { safe }));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(export_ini(loadouts, spell_names).as_bytes())?;
    Ok(path.display().to_string())
}

/// Desktop/EQLBuilder Exports — the one place the app writes user-facing files.
pub fn exports_dir() -> PathBuf {
    dirs_desktop().join("EQLBuilder Exports")
}

fn dirs_desktop() -> PathBuf {
    if let Ok(p) = std::env::var("EQL_DESKTOP_DIR") {
        return PathBuf::from(p);
    }
    // the known-folder API follows OneDrive redirection — USERPROFILE\Desktop does
    // NOT, and exports were landing in an invisible C:\Users\<u>\Desktop folder
    if let Some(d) = dirs::desktop_dir() {
        return d;
    }
    if let Ok(up) = std::env::var("USERPROFILE") {
        return PathBuf::from(up).join("Desktop");
    }
    PathBuf::from(".")
}

/// Reveal an exported file in Explorer (select it) so the user can never lose it.
pub fn reveal_in_explorer(path: &str) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
}

// ------------------------------------------------------ safe in-place merge (integrity)
//
// The real `<Char>_<city>_LO1.ini` is the character's ENTIRE settings file — [HotButtons]
// with item ids, [Socials], [Combat], [Defaults] sound, [MailOptions], … — with
// [SpellLoadouts] as just one section. Overwriting it with a naked fragment would wipe all
// of that, so writing loadouts back has to be a surgical merge: touch only the [SpellLoadouts]
// keys of the sets the build defines, and leave every other line — other sections AND other
// sets — exactly as the game wrote them. The game reads INI by key, so re-ordering the edited
// sets' keys (and dropping the game's own duplicate keys, e.g. a stray SpellLoadout3.slot14)
// is invisible to it.

/// "SpellLoadout12.slot3=..." -> Some(12); None if the line isn't a SpellLoadout key.
fn loadout_set_of(line: &str) -> Option<u32> {
    let key = line.trim().split('=').next()?;
    let (num, _field) = key.trim().strip_prefix("SpellLoadout")?.split_once('.')?;
    num.parse::<u32>().ok()
}

/// the newline the file already uses, so a rewrite never mixes \n and \r\n
fn detect_newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Outcome of a merge-write (surfaced so the UI can warn about dropped gems / show the backup).
#[derive(Debug, Clone, Serialize)]
pub struct LoadoutWrite {
    pub path: String,
    pub backup: Option<String>,
    pub sets_written: usize,
    /// gems that held a spell we couldn't map to a game id (written as empty -1)
    pub slots_unresolved: usize,
}

/// Overlay `loadouts` onto an existing char-INI's [SpellLoadouts] section, preserving every
/// other section verbatim and every set the build doesn't define. Sets 1..=loadouts.len() are
/// rewritten as clean canonical blocks (inuse=1, name, slot1..14); the game's own duplicate /
/// mis-prefixed keys for those sets are dropped. Returns (new_text, unresolved_gem_count).
pub fn merge_loadouts_into_char_ini(
    existing: &str,
    loadouts: &[SpellLoadout],
    spell_names: &BTreeMap<i64, String>,
) -> (String, usize) {
    merge_core(existing, loadouts, &wiki_to_game(spell_names))
}

/// The merge with the wiki->game id map supplied directly (hermetic core; tests hit this
/// without needing the client's spells_us.txt on disk).
fn merge_core(existing: &str, loadouts: &[SpellLoadout], w2g: &BTreeMap<i64, i64>) -> (String, usize) {
    let nl = detect_newline(existing);
    let edited: BTreeSet<u32> = (1..=loadouts.len() as u32).collect();

    // canonical clean block per edited set (in set order)
    let mut unresolved = 0usize;
    let mut blocks: Vec<String> = Vec::new();
    for (i, lo) in loadouts.iter().enumerate() {
        let n = (i + 1) as u32;
        blocks.push(format!("SpellLoadout{n}.inuse=1"));
        if !lo.name.is_empty() {
            blocks.push(format!("SpellLoadout{n}.name={}", lo.name));
        }
        for si in 0..eql_data::MAX_SPELL_GEMS {
            let held = lo.slots.get(si).copied().flatten();
            let gid = held.and_then(|wiki| w2g.get(&wiki).copied());
            if held.is_some() && gid.is_none() {
                unresolved += 1;
            }
            blocks.push(format!("SpellLoadout{n}.slot{}={}", si + 1, gid.unwrap_or(-1)));
        }
    }

    // work line-by-line without trailing CR/LF so we can rejoin with the detected newline
    let lines: Vec<&str> = existing.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let header = lines.iter().position(|l| l.trim().eq_ignore_ascii_case("[SpellLoadouts]"));

    let mut out: Vec<String> = Vec::new();
    match header {
        // no [SpellLoadouts] yet: keep the file, append a fresh section
        None => {
            out.extend(lines.iter().map(|l| l.to_string()));
            out.push("[SpellLoadouts]".to_string());
            out.extend(blocks);
        }
        Some(start) => {
            let end = lines.iter().enumerate().skip(start + 1)
                .find(|(_, l)| l.trim_start().starts_with('['))
                .map(|(i, _)| i)
                .unwrap_or(lines.len());
            out.extend(lines[..=start].iter().map(|l| l.to_string())); // up to & incl. header

            // body minus the edited sets' lines; remember where the first one was
            let mut body: Vec<String> = Vec::new();
            let mut insert_at: Option<usize> = None;
            for l in &lines[start + 1..end] {
                if loadout_set_of(l).is_some_and(|s| edited.contains(&s)) {
                    insert_at.get_or_insert(body.len());
                    continue; // drop; replaced by the clean block
                }
                body.push(l.to_string());
            }
            let at = insert_at.unwrap_or(0).min(body.len());
            body.splice(at..at, blocks);
            out.extend(body);
            out.extend(lines[end..].iter().map(|l| l.to_string())); // next section onward
        }
    }
    (out.join(nl), unresolved)
}

/// Read `target`, merge the loadouts into its [SpellLoadouts] (see above), back the original
/// up to `<name>.bak`, and write the result. Never touches any other section.
pub fn write_loadouts_to_char_ini(
    target: &Path,
    loadouts: &[SpellLoadout],
    spell_names: &BTreeMap<i64, String>,
) -> std::io::Result<LoadoutWrite> {
    let existing = std::fs::read_to_string(target)?;
    let (merged, slots_unresolved) = merge_loadouts_into_char_ini(&existing, loadouts, spell_names);
    let backup = target.with_extension("ini.bak");
    let backup = std::fs::copy(target, &backup).ok().map(|_| backup.display().to_string());
    std::fs::write(target, merged)?;
    Ok(LoadoutWrite {
        path: target.display().to_string(),
        backup,
        sets_written: loadouts.len(),
        slots_unresolved,
    })
}

/// A `<Char>_<city>_LO1.ini` settings file we can merge loadouts into.
#[derive(Debug, Clone, Serialize)]
pub struct LoadoutFile {
    pub path: String,
    pub name: String,
    pub character: Option<String>,
    pub city: Option<String>,
    pub set_count: usize, // sets currently marked inuse=1
    pub modified_epoch: u64,
}

/// "Testchar_qeynos_LO1.ini" -> (Some("Testchar"), Some("qeynos")).
fn parse_lo_filename(name: &str) -> (Option<String>, Option<String>) {
    let stem = name.strip_suffix(".ini").or_else(|| name.strip_suffix(".INI")).unwrap_or(name);
    let core = stem.strip_suffix("_LO1").or_else(|| stem.strip_suffix("_lo1")).unwrap_or(stem);
    match core.split_once('_') {
        Some((c, city)) if !c.is_empty() && !city.is_empty() => (Some(c.into()), Some(city.into())),
        _ => (None, None),
    }
}

/// count SpellLoadoutN.inuse=1 keys in a file's [SpellLoadouts] section
fn count_inuse(text: &str) -> usize {
    let mut in_section = false;
    let mut n = 0;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_section = l.eq_ignore_ascii_case("[SpellLoadouts]");
            continue;
        }
        if in_section {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().to_ascii_lowercase().ends_with(".inuse") && v.trim() == "1" {
                    n += 1;
                }
            }
        }
    }
    n
}

/// The real `<Char>_<city>_LO1.ini` settings files in the EQL folder (newest first).
/// Skips the `UI_*` window-layout files and the game's own `_Backup_*` copies.
pub fn list_loadout_files() -> Vec<LoadoutFile> {
    let Some(dir) = guess_eql_dir() else { return Vec::new() };
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with("_lo1.ini") || lower.starts_with("ui_") || lower.contains("_backup") {
                continue;
            }
            let path = entry.path();
            let (character, city) = parse_lo_filename(&name);
            let set_count = std::fs::read_to_string(&path).map(|t| count_inuse(&t)).unwrap_or(0);
            let modified_epoch = entry.metadata().and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()).unwrap_or(0);
            files.push(LoadoutFile { path: path.display().to_string(), name, character, city, set_count, modified_epoch });
        }
    }
    files.sort_by(|a, b| b.modified_epoch.cmp(&a.modified_epoch).then(a.name.cmp(&b.name)));
    files
}

/// Parse a loadout INI (or full char INI containing [SpellLoadouts]) into loadouts,
/// converting game spell ids -> wiki pageids by name. Slots that don't resolve become
/// None (so a round-trip through an unknown game id degrades to an empty gem).
pub fn import_ini(path: &Path, spell_names: &BTreeMap<i64, String>) -> std::io::Result<Vec<SpellLoadout>> {
    let text = std::fs::read_to_string(path)?;
    let game = game_spell_names();
    // game id -> wiki id, via name
    let name_to_wiki: BTreeMap<String, i64> =
        spell_names.iter().map(|(w, n)| (norm(n), *w)).collect();
    let g2w = |gid: i64| -> Option<i64> {
        game.get(&gid).and_then(|n| name_to_wiki.get(&norm(n)).copied())
    };

    // collect SpellLoadoutN.<key>=<val>
    let mut sets: BTreeMap<u32, (String, BTreeMap<u32, i64>)> = BTreeMap::new();
    let mut in_section = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_section = l.eq_ignore_ascii_case("[SpellLoadouts]");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let Some(rest) = k.strip_prefix("SpellLoadout") else { continue };
        let Some((num, field)) = rest.split_once('.') else { continue };
        let Ok(num) = num.parse::<u32>() else { continue };
        let entry = sets.entry(num).or_default();
        if field.eq_ignore_ascii_case("name") {
            entry.0 = v.to_string();
        } else if let Some(slot) = field.strip_prefix("slot") {
            if let (Ok(si), Ok(gid)) = (slot.parse::<u32>(), v.parse::<i64>()) {
                entry.1.insert(si, gid);
            }
        }
    }
    let mut out = Vec::new();
    for (_num, (name, slots)) in sets {
        if slots.is_empty() {
            continue;
        }
        let max = slots.keys().copied().max().unwrap_or(0).max(14);
        let mut vec = vec![None; max as usize];
        for (si, gid) in slots {
            if si >= 1 && gid > 0 {
                vec[(si - 1) as usize] = g2w(gid);
            }
        }
        out.push(SpellLoadout { name, slots: vec });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lo(name: &str, slots: Vec<Option<i64>>) -> SpellLoadout {
        SpellLoadout { name: name.into(), slots }
    }

    // a trimmed fixture with the real file's shape: sections around [SpellLoadouts], a
    // messy/duplicate loadout section (set 1 split, a stray SpellLoadout3.slot14), set 2 kept.
    const FIXTURE: &str = "[HotButtons]\r\n\
        Page1Button1=B0,@-1,0000,0,Melee<BR>Attack,\r\n\
        [SpellLoadouts]\r\n\
        SpellLoadout1.inuse=1\r\n\
        SpellLoadout1.name=OldName\r\n\
        SpellLoadout1.slot1=79\r\n\
        SpellLoadout1.slot2=1428\r\n\
        SpellLoadout3.slot14=-1\r\n\
        SpellLoadout2.inuse=1\r\n\
        SpellLoadout2.name=KeepMe\r\n\
        SpellLoadout2.slot1=46\r\n\
        [Socials]\r\n\
        Page2Button1Name=A Forage\r\n";

    #[test]
    fn merge_preserves_other_sections_and_untouched_sets() {
        // one edited set; game ids map 79->790, 1428->14280 (rest empty)
        let w2g = BTreeMap::from([(79i64, 790i64), (1428, 14280)]);
        let (out, unresolved) = merge_core(FIXTURE, &[lo("NewBuffs", vec![Some(79), Some(1428)])], &w2g);
        assert_eq!(unresolved, 0);

        // other sections survive verbatim
        assert!(out.contains("[HotButtons]\r\nPage1Button1=B0,@-1,0000,0,Melee<BR>Attack,"));
        assert!(out.contains("[Socials]\r\nPage2Button1Name=A Forage"));
        // set 2 (untouched) preserved exactly
        assert!(out.contains("SpellLoadout2.inuse=1"));
        assert!(out.contains("SpellLoadout2.name=KeepMe"));
        assert!(out.contains("SpellLoadout2.slot1=46"));
        // set 3's stray key is not ours to touch -> kept
        assert!(out.contains("SpellLoadout3.slot14=-1"));

        // set 1 rewritten cleanly: new name, mapped gems, all 14 slots, old name gone
        assert!(out.contains("SpellLoadout1.name=NewBuffs"));
        assert!(!out.contains("OldName"));
        assert!(out.contains("SpellLoadout1.slot1=790"));
        assert!(out.contains("SpellLoadout1.slot2=14280"));
        assert!(out.contains("SpellLoadout1.slot14=-1")); // padded
        // exactly one slot1 line for set 1 (no leftover duplicate)
        assert_eq!(out.matches("SpellLoadout1.slot1=").count(), 1);
        // newline style preserved
        assert!(out.contains("\r\n") && !out.contains("\n\n\n"));
    }

    #[test]
    fn merge_reports_unresolved_gems() {
        // slot holds wiki 999 which has no game-id mapping -> written -1, counted
        let w2g = BTreeMap::new();
        let (out, unresolved) = merge_core(FIXTURE, &[lo("X", vec![Some(999)])], &w2g);
        assert_eq!(unresolved, 1);
        assert!(out.contains("SpellLoadout1.slot1=-1"));
    }

    #[test]
    fn merge_editing_two_sets_rewrites_both() {
        let w2g = BTreeMap::new();
        let (out, _) = merge_core(FIXTURE, &[lo("A", vec![]), lo("B", vec![])], &w2g);
        assert!(out.contains("SpellLoadout1.name=A"));
        assert!(out.contains("SpellLoadout2.name=B"));
        assert!(!out.contains("KeepMe")); // set 2 was edited, old name replaced
    }

    #[test]
    fn merge_appends_section_when_absent() {
        let no_section = "[HotButtons]\nPage1Button1=B0\n";
        let (out, _) = merge_core(no_section, &[lo("Only", vec![])], &BTreeMap::new());
        assert!(out.contains("[HotButtons]\nPage1Button1=B0"));
        assert!(out.contains("[SpellLoadouts]\nSpellLoadout1.inuse=1"));
    }

    #[test]
    fn lo_filename_parsed_and_ui_backups_excluded() {
        assert_eq!(parse_lo_filename("Testchar_qeynos_LO1.ini"),
                   (Some("Testchar".into()), Some("qeynos".into())));
        // count_inuse reads the real section shape
        assert_eq!(count_inuse(FIXTURE), 2);
    }

    /// text with the whole [SpellLoadouts] section blanked out — used to prove a merge
    /// leaves everything OUTSIDE that section untouched.
    fn without_loadouts(text: &str) -> String {
        let mut out = String::new();
        let mut skip = false;
        for line in text.split_inclusive('\n') {
            let t = line.trim();
            if t.starts_with('[') { skip = t.eq_ignore_ascii_case("[SpellLoadouts]"); }
            if !skip { out.push_str(line); }
        }
        out
    }

    // Definitive integrity check against a REAL char INI. Point EQL_TEST_LO1 at any
    // `<Char>_<city>_LO1.ini`; the test skips cleanly when it's unset or missing, so this
    // stays portable. Proves the merge never disturbs any other section — hotbars, socials,
    // sound, mail — the whole point of task 77.
    #[test]
    fn real_file_merge_touches_only_spellloadouts() {
        let Ok(path) = std::env::var("EQL_TEST_LO1") else { return };
        let Ok(before) = std::fs::read_to_string(&path) else { return };
        // map nothing (slots -> -1); we only care about structural integrity here
        let (after, _) = merge_core(
            &before,
            &[lo("Buffs", vec![]), lo("Normal", vec![])],
            &BTreeMap::new(),
        );
        // everything that is NOT the [SpellLoadouts] section must be byte-identical
        assert_eq!(without_loadouts(&before), without_loadouts(&after),
                   "a non-loadout section changed — merge is unsafe");
        // and the edited sets are present in the result
        assert!(after.contains("SpellLoadout1.name=Buffs"));
        assert!(after.contains("SpellLoadout2.name=Normal"));
    }
}
