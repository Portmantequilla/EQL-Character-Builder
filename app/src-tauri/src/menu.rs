//! Native menu bar. Menu clicks are forwarded to the webview as a `menu` event
//! carrying the item id; the frontend owns what each one does (it holds the build
//! state). Keep ids stable — the frontend switches on them.
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: &str, label: &str, accel: Option<&str>| {
        let mut b = MenuItemBuilder::with_id(id, label);
        if let Some(a) = accel {
            b = b.accelerator(a);
        }
        b.build(app)
    };

    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new_build", "New Build", Some("CmdOrCtrl+N"))?)
        .item(&item("open_build", "Open Build…", Some("CmdOrCtrl+O"))?)
        .item(&item("save_build", "Save Build", Some("CmdOrCtrl+S"))?)
        .separator()
        .item(&item("export_build", "Export Build (share)…", Some("CmdOrCtrl+E"))?)
        .item(&item("import_build", "Import Build…", Some("CmdOrCtrl+I"))?)
        .separator()
        .item(&item("import_spellbook", "Import Spell Sets…", None)?)
        .item(&item("export_spellbook", "Export Spell Sets to Desktop", None)?)
        .separator()
        .item(&item("settings", "Settings (formulas)…", Some("CmdOrCtrl+,"))?)
        .item(&item("open_data_folder", "Open Data Folder", None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Exit"))?)
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .separator()
        .item(&item("choose_for_me", "Choose For Me (random build)", None)?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("tab_overview", "Overview", Some("CmdOrCtrl+1"))?)
        .item(&item("tab_equipment", "Equipment", Some("CmdOrCtrl+2"))?)
        .item(&item("tab_buffs", "Buffs", Some("CmdOrCtrl+3"))?)
        .item(&item("tab_spells", "Spell Info", Some("CmdOrCtrl+4"))?)
        .item(&item("tab_spellbook", "Spell Manager", Some("CmdOrCtrl+5"))?)
        .item(&item("tab_stats", "Stats", Some("CmdOrCtrl+6"))?)
        .item(&item("tab_pet", "Pet", Some("CmdOrCtrl+7"))?)
        .item(&item("tab_farm", "Farm", Some("CmdOrCtrl+8"))?)
        .item(&item("tab_lootfilter", "Loot Filter", Some("CmdOrCtrl+9"))?)
        .item(&item("tab_macros", "Macros", None)?)
        .separator()
        .item(&item("reload", "Reload", Some("F5"))?)
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&item("verification", "Data & Verification Notes", None)?)
        .item(&item("wiki", "EQL Wiki (eqlwiki.com)", None)?)
        .separator()
        .item(&item("about", "About EQL Character Builder", None)?)
        .build()?;

    MenuBuilder::new(app)
        .items(&[&file, &edit, &view, &help])
        .build()
}

/// One handler for every menu id: forward to the webview, which owns build state.
pub fn on_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let _ = app.emit("menu", id.to_string());
}

// The About dialog is rendered in the webview (AboutDialog.svelte) rather than the
// OS panel, so it can carry the full credits + legal text and the data paths.
