//! Read/write the game's social macros — the `[Socials]` section of `<Char>_<city>_LO1.ini`.
//!
//! A social is a button at `Page<P>Button<B>` with a Name, a Color (0-15 chat palette), and up
//! to 5 command Lines (`Line1`..`Line5`; the in-game Edit Social window shows five line fields).
//! Example:
//!   Page2Button7Name=AT
//!   Page2Button7Color=13
//!   Page2Button7Line1=/doability forage
//!   Page2Button7Line2=/target Fippy Darkpaw
//!   ...
//!
//! Writing back replaces ONLY the `[Socials]` section and preserves every other section
//! (spell loadouts, hotbuttons, sound, mail…) verbatim — the same integrity rule the spellbook
//! merge follows. The macro tab holds the file's COMPLETE social set (it loads them all first),
//! so a full-section replace is correct and makes deletes take effect.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// up to this many command lines per social (the Edit Social window's five fields)
pub const MAX_SOCIAL_LINES: usize = 5;

/// One social macro (a button in the in-game Socials window).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Social {
    pub page: u32,
    pub button: u32,
    pub name: String,
    pub color: i64,
    /// command lines in order (trailing blanks trimmed); at most MAX_SOCIAL_LINES
    pub lines: Vec<String>,
}

fn detect_newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// "Page2Button7Line3" -> Some((2, 7, "Line3")). None if the key isn't a social button key.
fn parse_social_key(key: &str) -> Option<(u32, u32, &str)> {
    let rest = key.strip_prefix("Page")?;
    let (page_s, rest) = rest.split_once("Button")?;
    let page = page_s.parse::<u32>().ok()?;
    // rest = "<button><field>", e.g. "7Line3" / "7Name" / "7Color". Split the leading digits.
    let split = rest.find(|c: char| !c.is_ascii_digit())?;
    let button = rest[..split].parse::<u32>().ok()?;
    Some((page, button, &rest[split..]))
}

/// Parse the `[Socials]` section into a list of socials (sorted by page then button).
pub fn parse_socials(text: &str) -> Vec<Social> {
    // (page,button) -> (name, color, {line index -> text})
    let mut acc: BTreeMap<(u32, u32), (String, i64, BTreeMap<u32, String>)> = BTreeMap::new();
    let mut in_section = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_section = l.eq_ignore_ascii_case("[Socials]");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, val)) = l.split_once('=') else { continue };
        let Some((page, button, field)) = parse_social_key(key.trim()) else { continue };
        let entry = acc.entry((page, button)).or_default();
        if field.eq_ignore_ascii_case("Name") {
            entry.0 = val.to_string();
        } else if field.eq_ignore_ascii_case("Color") {
            entry.1 = val.trim().parse::<i64>().unwrap_or(0);
        } else if let Some(idx) = field.strip_prefix("Line").or_else(|| field.strip_prefix("line")) {
            if let Ok(n) = idx.parse::<u32>() {
                entry.2.insert(n, val.to_string());
            }
        }
    }
    acc.into_iter()
        .map(|((page, button), (name, color, line_map))| {
            // flatten Line1..LineN in order, trimming trailing blanks, capping at MAX
            let max = line_map.keys().copied().max().unwrap_or(0);
            let mut lines: Vec<String> = (1..=max)
                .map(|i| line_map.get(&i).cloned().unwrap_or_default())
                .collect();
            while lines.last().is_some_and(|s| s.is_empty()) {
                lines.pop();
            }
            lines.truncate(MAX_SOCIAL_LINES);
            Social { page, button, name, color, lines }
        })
        // drop fully-empty buttons (no name and no lines) — they aren't real socials
        .filter(|s| !s.name.is_empty() || !s.lines.is_empty())
        .collect()
}

/// The `[Socials]` body as key=value lines (no header), canonical order.
fn socials_body(socials: &[Social]) -> Vec<String> {
    let mut out = Vec::new();
    let mut sorted: Vec<&Social> = socials.iter().collect();
    sorted.sort_by(|a, b| a.page.cmp(&b.page).then(a.button.cmp(&b.button)));
    for s in sorted {
        if s.name.is_empty() && s.lines.iter().all(|l| l.is_empty()) {
            continue; // skip empties
        }
        let p = format!("Page{}Button{}", s.page, s.button);
        out.push(format!("{p}Name={}", s.name));
        out.push(format!("{p}Color={}", s.color));
        for (i, line) in s.lines.iter().enumerate().take(MAX_SOCIAL_LINES) {
            if !line.is_empty() {
                out.push(format!("{p}Line{}={line}", i + 1));
            }
        }
    }
    out
}

/// Replace the whole `[Socials]` section of a char INI with the given socials, preserving every
/// other section verbatim and the file's newline style. Appends the section if it's absent.
pub fn merge_socials_into_char_ini(existing: &str, socials: &[Social]) -> String {
    let nl = detect_newline(existing);
    let body = socials_body(socials);
    let lines: Vec<&str> = existing.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let header = lines.iter().position(|l| l.trim().eq_ignore_ascii_case("[Socials]"));

    let mut out: Vec<String> = Vec::new();
    match header {
        None => {
            out.extend(lines.iter().map(|l| l.to_string()));
            out.push("[Socials]".to_string());
            out.extend(body);
        }
        Some(start) => {
            let end = lines.iter().enumerate().skip(start + 1)
                .find(|(_, l)| l.trim_start().starts_with('['))
                .map(|(i, _)| i)
                .unwrap_or(lines.len());
            out.extend(lines[..=start].iter().map(|l| l.to_string())); // up to & incl. header
            out.extend(body); // the new section body replaces the old
            out.extend(lines[end..].iter().map(|l| l.to_string())); // next section onward
        }
    }
    out.join(nl)
}

/// Outcome of a socials write.
#[derive(Debug, Clone, Serialize)]
pub struct SocialWrite {
    pub path: String,
    pub backup: Option<String>,
    pub count: usize,
}

/// Read `target`, replace its `[Socials]` with `socials`, back up to `<name>.bak`, and write.
pub fn write_socials_to_char_ini(target: &Path, socials: &[Social]) -> std::io::Result<SocialWrite> {
    let existing = std::fs::read_to_string(target)?;
    let merged = merge_socials_into_char_ini(&existing, socials);
    let backup = target.with_extension("ini.bak");
    let backup = std::fs::copy(target, &backup).ok().map(|_| backup.display().to_string());
    std::fs::write(target, merged)?;
    Ok(SocialWrite {
        path: target.display().to_string(),
        backup,
        count: socials.iter().filter(|s| !s.name.is_empty() || !s.lines.is_empty()).count(),
    })
}

/// Read socials from a LO1 file (or any INI/fragment with a `[Socials]` section).
pub fn read_socials_file(path: &Path) -> std::io::Result<Vec<Social>> {
    let bytes = std::fs::read(path)?;
    Ok(parse_socials(&String::from_utf8_lossy(&bytes)))
}

/// A paste-ready `[Socials]` fragment (header + body) for sharing or backup. Reads back cleanly
/// through `read_socials_file` / `parse_socials`.
pub fn socials_fragment(socials: &[Social]) -> String {
    let mut out = String::from("[Socials]\n");
    for line in socials_body(socials) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "[HotButtons]\r\n\
        Page1Button1=B0\r\n\
        [Socials]\r\n\
        Page2Button1Name=A Forage\r\n\
        Page2Button1Color=0\r\n\
        Page2Button1Line1=/doability Forage\r\n\
        Page2Button1Line2=/autoinventory\r\n\
        Page2Button7Name=AT\r\n\
        Page2Button7Color=13\r\n\
        Page2Button7Line1=/doability forage\r\n\
        Page2Button7Line2=/target Fippy Darkpaw\r\n\
        Page2Button7Line3=/attack\r\n\
        Page2Button7Line4=/pause 15\r\n\
        Page2Button7Line5=/autoinventory\r\n\
        [MailOptions]\r\n\
        NewMailNotificationInChatWindow=1\r\n";

    #[test]
    fn parses_socials_with_pages_colors_lines() {
        let s = parse_socials(FIXTURE);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].page, 2);
        assert_eq!(s[0].button, 1);
        assert_eq!(s[0].name, "A Forage");
        assert_eq!(s[0].color, 0);
        assert_eq!(s[0].lines, vec!["/doability Forage", "/autoinventory"]);
        assert_eq!(s[1].button, 7);
        assert_eq!(s[1].color, 13);
        assert_eq!(s[1].lines.len(), 5);
        assert_eq!(s[1].lines[1], "/target Fippy Darkpaw");
    }

    #[test]
    fn merge_replaces_only_socials_and_keeps_others() {
        let new = vec![Social {
            page: 1, button: 1, name: "Buff".into(), color: 4,
            lines: vec!["/cast 1".into(), "/pause 30".into()],
        }];
        let out = merge_socials_into_char_ini(FIXTURE, &new);
        // other sections byte-preserved
        assert!(out.contains("[HotButtons]\r\nPage1Button1=B0"));
        assert!(out.contains("[MailOptions]\r\nNewMailNotificationInChatWindow=1"));
        // old socials gone, new one present
        assert!(!out.contains("A Forage") && !out.contains("Fippy"));
        assert!(out.contains("Page1Button1Name=Buff"));
        assert!(out.contains("Page1Button1Color=4"));
        assert!(out.contains("Page1Button1Line2=/pause 30"));
        assert!(out.contains("\r\n") && !out.contains("\n\n\n"));
    }

    #[test]
    fn round_trips_names_colors_lines() {
        let parsed = parse_socials(FIXTURE);
        let out = merge_socials_into_char_ini(FIXTURE, &parsed);
        let reparsed = parse_socials(&out);
        assert_eq!(reparsed.len(), 2);
        assert_eq!(reparsed[1].lines, parsed[1].lines);
        assert_eq!(reparsed[0].name, "A Forage");
    }

    #[test]
    fn appends_section_when_absent() {
        let no_sec = "[HotButtons]\nPage1Button1=B0\n";
        let out = merge_socials_into_char_ini(
            no_sec,
            &[Social { page: 1, button: 1, name: "X".into(), color: 0, lines: vec!["/say hi".into()] }],
        );
        assert!(out.contains("[HotButtons]\nPage1Button1=B0"));
        assert!(out.contains("[Socials]\nPage1Button1Name=X"));
    }

    #[test]
    fn trailing_blank_lines_trimmed_on_parse() {
        let text = "[Socials]\nPage1Button1Name=Z\nPage1Button1Color=0\n\
                    Page1Button1Line1=/a\nPage1Button1Line2=\nPage1Button1Line3=\n";
        let s = parse_socials(text);
        assert_eq!(s[0].lines, vec!["/a"]); // the two blank trailing lines dropped
    }
}
