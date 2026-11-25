//! Importing beatmaps, skins, replays and osu:// links.

use crate::config::Config;
use crate::launch::{game_command, windows_path};
use crate::wine::{have, Wine};
use crate::{system, ui};
use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Item {
    Beatmap(PathBuf),
    Skin(PathBuf),
    Replay(PathBuf),
    Link(String),
}

pub fn classify(arg: &str) -> Option<Item> {
    if arg.starts_with("osu://") {
        return Some(Item::Link(arg.to_string()));
    }
    let path = PathBuf::from(arg.strip_prefix("file://").unwrap_or(arg));
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "osz" | "olz" => Some(Item::Beatmap(path)),
        "osk" => Some(Item::Skin(path)),
        "osr" => Some(Item::Replay(path)),
        _ => None,
    }
}

/// Unpacks a .osk into Skins/<name>, refusing entries that would escape the folder.
fn extract_skin(osk: &Path, skins: &Path) -> Result<PathBuf> {
    let name = osk.file_stem().context("skin file has no name")?;
    let dest = skins.join(name);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(osk)?)
        .with_context(|| format!("{} is not a valid skin archive", osk.display()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            std::fs::create_dir_all(out.parent().unwrap())?;
            std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
        }
    }
    Ok(dest)
}

/// Hands one argument to the running client: a second osu! process forwards it and exits.
fn forward(cfg: &Config, wine: &Wine, arg: &str) -> Result<()> {
    let (mut cmd, _) = game_command(cfg, wine, &[arg.to_string()])?;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(40) {
            child.kill().ok();
            bail!("osu! did not accept the import in time");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn wait_for_game(settle: Duration) -> Result<()> {
    let start = Instant::now();
    while !system::osu_running() {
        if start.elapsed() > Duration::from_secs(60) {
            bail!("osu! did not start");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // The client only accepts imports once its menu is up.
    std::thread::sleep(settle);
    Ok(())
}

fn notify(summary: &str) {
    if !std::io::stderr().is_terminal() && have("notify-send") {
        Command::new("notify-send").args(["-a", "osu!", "osu! import", summary]).status().ok();
    }
}

pub fn import(cfg: &Config, args: &[String]) -> Result<()> {
    if !cfg.installed() {
        bail!("osu! is not installed yet. Run `stdlinux install`.");
    }
    let wine = Wine::locate();
    let mut items = vec![];
    for a in args {
        match classify(a) {
            Some(Item::Link(l)) => items.push(Item::Link(l)),
            Some(item) => {
                let (Item::Beatmap(p) | Item::Skin(p) | Item::Replay(p)) = &item else { unreachable!() };
                if p.is_file() {
                    items.push(item);
                } else {
                    ui::fail(format!("{}: no such file", p.display()));
                }
            }
            None => ui::fail(format!("{a}: not a .osz, .osk, .osr file or osu:// link")),
        }
    }
    if items.is_empty() {
        bail!("nothing to import");
    }

    let mut running = system::osu_running();
    let mut done = 0;
    // Beatmaps and skins can be placed on disk directly, so they never need the game.
    // Replays and links can only be opened by a running client.
    let needs_game = items.iter().any(|i| matches!(i, Item::Replay(_) | Item::Link(_)));
    if needs_game && !running {
        ui::info("starting osu! to open the replay/link");
        Command::new(std::env::current_exe()?)
            .arg("run")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("cannot start the game")?;
        let settle = std::env::var("STDLINUX_SETTLE").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
        wait_for_game(Duration::from_secs(settle))?;
        running = true;
    }

    for item in &items {
        let res: Result<String> = match item {
            Item::Beatmap(p) if running => forward(cfg, &wine, &windows_path(&p.canonicalize()?))
                .map(|_| format!("{} sent to osu!", name(p))),
            Item::Beatmap(p) => {
                let songs = cfg.osu_dir.join("Songs");
                std::fs::create_dir_all(&songs)?;
                std::fs::copy(p, songs.join(p.file_name().unwrap()))
                    .map(|_| format!("{} added (osu! imports it on next start)", name(p)))
                    .map_err(Into::into)
            }
            Item::Skin(p) if running => forward(cfg, &wine, &windows_path(&p.canonicalize()?))
                .map(|_| format!("{} sent to osu!", name(p))),
            Item::Skin(p) => extract_skin(p, &cfg.osu_dir.join("Skins"))
                .map(|d| format!("skin installed to {}", d.display())),
            Item::Replay(p) => forward(cfg, &wine, &windows_path(&p.canonicalize()?))
                .map(|_| format!("{} opened", name(p))),
            Item::Link(l) => forward(cfg, &wine, l).map(|_| format!("{l} opened")),
        };
        match res {
            Ok(msg) => {
                done += 1;
                ui::ok(msg);
            }
            Err(e) => ui::fail(format!("{e:#}")),
        }
    }
    notify(&format!("{done} of {} imported", items.len()));
    if done < items.len() {
        bail!("{} item(s) failed", items.len() - done);
    }
    Ok(())
}

fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn classifies_by_extension_and_scheme() {
        assert_eq!(classify("/a/b.OSZ"), Some(Item::Beatmap("/a/b.OSZ".into())));
        assert_eq!(classify("file:///a/skin.osk"), Some(Item::Skin("/a/skin.osk".into())));
        assert_eq!(classify("r.osr"), Some(Item::Replay("r.osr".into())));
        assert_eq!(classify("osu://b/123"), Some(Item::Link("osu://b/123".into())));
        assert_eq!(classify("notes.txt"), None);
        assert_eq!(classify("noext"), None);
    }

    #[test]
    fn skin_extraction_stays_inside_the_skin_folder() {
        let tmp = std::env::temp_dir().join(format!("stdlinux-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let osk = tmp.join("Cool Skin.osk");
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&osk).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("skin.ini", opts).unwrap();
        zw.write_all(b"[General]").unwrap();
        zw.start_file("../../escape.txt", opts).unwrap();
        zw.write_all(b"x").unwrap();
        zw.finish().unwrap();

        let dest = extract_skin(&osk, &tmp.join("Skins")).unwrap();
        assert_eq!(dest, tmp.join("Skins/Cool Skin"));
        assert!(dest.join("skin.ini").is_file());
        assert!(!tmp.join("escape.txt").exists());
        assert!(!tmp.parent().unwrap().join("escape.txt").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
