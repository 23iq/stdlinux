//! App-menu entry, file associations for .osz/.osk/.osr and the osu:// scheme.

use crate::config::share_dir;
use crate::ui;
use crate::wine::have;
use anyhow::Result;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const GAME_ENTRY: &str = "stdlinux-osu.desktop";
const IMPORT_ENTRY: &str = "stdlinux-osu-import.desktop";
const MIME_FILE: &str = "stdlinux-osu.xml";

pub const MIME_TYPES: &[(&str, &str, &[&str])] = &[
    ("application/x-osu-beatmap-archive", "osu! beatmap", &["*.osz", "*.olz"]),
    ("application/x-osu-skin-archive", "osu! skin", &["*.osk"]),
    ("application/x-osu-replay", "osu! replay", &["*.osr"]),
];

const ALIAS_TYPES: &[&str] = &["application/x-osu-beatmap", "application/x-osu-skin", "application/x-osu-replay"];

/// How desktop files should call this tool: by name when it is on PATH (survives
/// upgrades and Nix store path changes), by absolute path otherwise.
fn self_command() -> String {
    if have("stdlinux") {
        return "stdlinux".into();
    }
    std::env::current_exe()
        .map(|p| format!("\"{}\"", p.display()))
        .unwrap_or_else(|_| "stdlinux".into())
}

fn paths() -> (PathBuf, PathBuf) {
    (share_dir().join("applications"), share_dir().join("mime/packages"))
}

fn quiet(cmd: &str, args: &[&str]) {
    if have(cmd) {
        Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().ok();
    }
}

pub fn integrate() -> Result<()> {
    let (apps, mime) = paths();
    std::fs::create_dir_all(&apps)?;
    std::fs::create_dir_all(&mime)?;
    let me = self_command();

    std::fs::write(
        apps.join(GAME_ENTRY),
        format!(
            "[Desktop Entry]\nType=Application\nName=osu!\nComment=osu! stable (stdlinux)\nExec={me} run\nIcon=applications-games\nCategories=Game;\nStartupWMClass=osu!.exe\nTerminal=false\n"
        ),
    )?;
    let mut types: Vec<&str> = MIME_TYPES.iter().map(|t| t.0).collect();
    // Other installers register the same files under these names; whichever definition
    // the system resolves a file to, the importer has to be its handler.
    types.extend(ALIAS_TYPES);
    std::fs::write(
        apps.join(IMPORT_ENTRY),
        format!(
            "[Desktop Entry]\nType=Application\nName=osu! importer\nExec={me} import %U\nIcon=applications-games\nNoDisplay=true\nTerminal=false\nMimeType={};x-scheme-handler/osu;\n",
            types.join(";")
        ),
    )?;

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n");
    for (ty, comment, globs) in MIME_TYPES {
        xml.push_str(&format!("  <mime-type type=\"{ty}\">\n    <comment>{comment}</comment>\n"));
        for g in *globs {
            xml.push_str(&format!("    <glob pattern=\"{g}\"/>\n"));
        }
        xml.push_str("  </mime-type>\n");
    }
    xml.push_str("</mime-info>\n");
    std::fs::write(mime.join(MIME_FILE), xml)?;

    let mime_root = share_dir().join("mime");
    quiet("update-mime-database", &[&mime_root.to_string_lossy()]);
    quiet("update-desktop-database", &[&apps.to_string_lossy()]);
    for ty in types.iter().chain(["x-scheme-handler/osu"].iter()) {
        quiet("xdg-mime", &["default", IMPORT_ENTRY, ty]);
    }
    ui::ok("app-menu entry added");
    if have("xdg-mime") {
        ui::ok(".osz, .osk, .osr files and osu:// links open with stdlinux");
    } else {
        ui::warn("xdg-mime not found: file associations were written but not made the default");
    }
    Ok(())
}

pub fn remove() -> Result<()> {
    let (apps, mime) = paths();
    for p in [apps.join(GAME_ENTRY), apps.join(IMPORT_ENTRY), mime.join(MIME_FILE)] {
        if p.is_file() {
            std::fs::remove_file(p)?;
        }
    }
    quiet("update-mime-database", &[&share_dir().join("mime").to_string_lossy()]);
    quiet("update-desktop-database", &[&apps.to_string_lossy()]);
    Ok(())
}
