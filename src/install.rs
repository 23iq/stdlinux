//! `stdlinux install`: Wine, prefix, the game, desktop integration.

use crate::config::{data_dir, presets, Config};
use crate::wine::{download, have, sha256_file, Wine};
use crate::{desktop, doctor, system, ui};
use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const OSU_INSTALLER_URL: &str = "https://m1.ppy.sh/r/osu!install.exe";
const WINETRICKS_URL: &str = "https://raw.githubusercontent.com/Winetricks/winetricks/20250102/src/winetricks";
const WINETRICKS_SHA256: &str = "53194dead910f8a5eb1deacaa4773d4e48f5873633d18ab1ecd6fdb0cb92243b";

pub struct Options {
    /// Never ask; take the first (recommended) choice everywhere.
    pub yes: bool,
    /// Ignore existing installs and build everything from scratch.
    pub fresh: bool,
    pub preset: Option<String>,
    pub osu_dir: Option<PathBuf>,
}

/// An osu! setup made by another tool that can be reused.
struct Existing {
    prefix: PathBuf,
    osu_dir: Option<PathBuf>,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap())
}

fn has_dotnet(prefix: &Path) -> bool {
    prefix.join("drive_c/windows/Microsoft.NET/Framework64/v4.0.30319/mscorlib.dll").is_file()
}

fn find_osu_in_prefix(prefix: &Path) -> Option<PathBuf> {
    for user in std::fs::read_dir(prefix.join("drive_c/users")).ok()?.flatten() {
        let dir = user.path().join("AppData/Local/osu!");
        if dir.join("osu!.exe").is_file() {
            return Some(dir);
        }
    }
    None
}

fn find_existing() -> Option<Existing> {
    let h = home();
    let prefixes = [h.join(".wine-osu"), h.join(".local/share/wineprefixes/osu-wineprefix")];
    let osu_dirs = [h.join(".local/share/osu-wine/osu!")];
    for prefix in prefixes {
        if prefix.join("system.reg").is_file() && has_dotnet(&prefix) {
            let osu_dir = find_osu_in_prefix(&prefix)
                .or_else(|| osu_dirs.iter().find(|d| d.join("osu!.exe").is_file()).cloned());
            return Some(Existing { prefix, osu_dir });
        }
    }
    None
}

fn choose(prompt: &str, items: &[&str], yes: bool) -> Result<usize> {
    if yes || !std::io::stdin().is_terminal() {
        return Ok(0);
    }
    Ok(dialoguer::Select::new().with_prompt(format!("  {prompt}")).items(items).default(0).interact()?)
}

/// Copies a directory tree keeping symlinks (a prefix's dosdevices are symlinks), skipping `skip`.
fn copy_tree(src: &Path, dst: &Path, skip: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src).with_context(|| format!("cannot read {}", src.display()))? {
        let e = e?;
        let from = e.path();
        if skip == Some(from.as_path()) {
            continue;
        }
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else if ft.is_dir() {
            copy_tree(&from, &to, skip)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to).with_context(|| format!("cannot copy {}", from.display()))?;
        }
    }
    Ok(())
}

fn distro_hint(packages: &str) -> String {
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
    let id = |name: &str| os.lines().any(|l| (l.starts_with("id=") || l.starts_with("id_like=")) && l.contains(name));
    if id("nixos") {
        "use the flake: nix run github:23iq/stdlinux -- install".to_string()
    } else if id("arch") {
        format!("sudo pacman -S {packages}")
    } else if id("debian") || id("ubuntu") {
        format!("sudo apt install {packages}")
    } else if id("fedora") || id("rhel") {
        format!("sudo dnf install {packages}")
    } else if id("suse") {
        format!("sudo zypper install {packages}")
    } else if id("void") {
        format!("sudo xbps-install {packages}")
    } else {
        format!("install with your package manager: {packages}")
    }
}

/// Runs a setup step with its output in a log file. Wine's display variables are removed:
/// a setup step that can open a window can also open an error dialog, which would sit on
/// the user's screen and block the install until someone clicks it.
fn run_logged(mut cmd: Command, what: &str, log_name: &str, limit: Duration) -> Result<()> {
    let logs = data_dir().join("logs");
    std::fs::create_dir_all(&logs)?;
    let path = logs.join(log_name);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    let sp = ui::spinner(what.to_string());
    let spawned = cmd
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            sp.finish_and_clear();
            bail!("{what}: cannot start: {e}");
        }
    };
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > limit {
            child.kill().ok();
            sp.finish_and_clear();
            bail!("{what} did not finish within {} minutes; log: {}", limit.as_secs() / 60, path.display());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    sp.finish_and_clear();
    if !status.success() {
        bail!("{what} failed ({status}); log: {}", path.display());
    }
    ui::ok(what);
    Ok(())
}

/// osu! is a 32-bit program: a prefix that cannot start one is useless, whatever wineboot said.
fn check_32bit(cfg: &Config, wine: &Wine) -> Result<()> {
    let out = wine
        .cmd("wine", cfg)
        .args(["C:\\windows\\syswow64\\cmd.exe", "/c", "echo", "stdlinux-ok"])
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("cannot run Wine")?;
    wine.cmd("wineserver", cfg).arg("-w").status().ok();
    if !String::from_utf8_lossy(&out.stdout).contains("stdlinux-ok") {
        bail!("the prefix cannot run 32-bit programs, which osu! is. Try `stdlinux install --fresh`.");
    }
    ui::ok("32-bit programs run");
    Ok(())
}

fn winetricks_path() -> Result<PathBuf> {
    let cache = data_dir().join("cache");
    std::fs::create_dir_all(&cache)?;
    let path = cache.join("winetricks");
    if !path.is_file() || sha256_file(&path)? != WINETRICKS_SHA256 {
        download(WINETRICKS_URL, &path, "winetricks")?;
        if sha256_file(&path)? != WINETRICKS_SHA256 {
            std::fs::remove_file(&path).ok();
            bail!("winetricks download does not match its pinned checksum");
        }
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

fn build_fresh_prefix(cfg: &Config, wine: &Wine) -> Result<()> {
    let missing: Vec<&str> = ["cabextract", "unzip"].into_iter().filter(|t| !have(t)).collect();
    if !missing.is_empty() {
        bail!(
            "building a prefix needs: {}\n  {}",
            missing.join(", "),
            distro_hint(&missing.join(" "))
        );
    }
    let mut boot = wine.cmd("wineboot", cfg);
    // No Mono/Gecko install prompts: the real .NET goes in next.
    boot.arg("-i").env("WINEDLLOVERRIDES", "mscoree,mshtml=;winemenubuilder.exe=");
    run_logged(boot, "Creating Wine prefix", "prefix.log", Duration::from_secs(300))?;
    wine.cmd("wineserver", cfg).arg("-w").status().ok();

    let shims = wine.shim_dir()?;
    let mut wt = Command::new(winetricks_path()?);
    wt.args(["-q", "dotnet48", "gdiplus"])
        .env("WINEPREFIX", &cfg.prefix)
        .env("WINE", shims.join("wine"))
        // The shims are scripts, so winetricks cannot read an architecture off them and
        // would end up with an empty command. This build is new-style WoW64: one binary.
        .env("WINE64", shims.join("wine"))
        .env("WINESERVER", shims.join("wineserver"))
        .env("WINEDEBUG", "-all")
        .env("WINEDLLOVERRIDES", "winemenubuilder.exe=");
    run_logged(wt, "Installing .NET Framework 4.8 (takes several minutes)", "prefix.log", Duration::from_secs(2400))?;
    wine.cmd("wineserver", cfg).arg("-w").status().ok();
    Ok(())
}

pub fn install(mut cfg: Config, opts: Options) -> Result<()> {
    ui::banner("install");
    if let Some(p) = &opts.preset {
        if !presets().iter().any(|x| x.name == p) {
            bail!("unknown preset '{p}'");
        }
        cfg.preset = p.clone();
    }
    for tool in ["tar", "xz"] {
        if !have(tool) {
            bail!("'{tool}' is required\n  {}", distro_hint("tar xz"));
        }
    }
    let existing = if opts.fresh { None } else { find_existing() };

    ui::section("Wine");
    let wine = Wine::locate();
    wine.ensure()?;
    match wine.version(&cfg) {
        Ok(v) => ui::ok(format!("{v}{}", if wine.wrap.is_empty() { String::new() } else { format!(" (via {})", wine.wrap.join(" ")) })),
        Err(e) => bail!("the Wine build does not start: {e}"),
    }

    ui::section("Prefix");
    let started = cfg.prefix.join("system.reg").is_file();
    if started && has_dotnet(&cfg.prefix) {
        ui::ok(format!("already present at {}", cfg.prefix.display()));
    } else if started {
        // An earlier install was interrupted before .NET went in.
        ui::info("finishing an incomplete prefix");
        build_fresh_prefix(&cfg, &wine)?;
    } else {
        let reuse = match &existing {
            Some(ex) => {
                ui::info(format!("found an existing osu! prefix with .NET at {}", ex.prefix.display()));
                choose(
                    "How should the prefix be set up?",
                    &["Copy the existing one (fast, the original is left untouched)", "Build a fresh one (downloads .NET 4.8)"],
                    opts.yes,
                )? == 0
            }
            None => false,
        };
        if reuse {
            let ex = existing.as_ref().unwrap();
            let sp = ui::spinner("Copying prefix");
            // The game folder can hold tens of gigabytes of songs; it is never copied.
            let res = copy_tree(&ex.prefix, &cfg.prefix, ex.osu_dir.as_deref());
            sp.finish_and_clear();
            res?;
            ui::ok("prefix copied");
        } else {
            build_fresh_prefix(&cfg, &wine)?;
        }
    }
    let mut update = wine.cmd("wineboot", &cfg);
    update.arg("-u");
    run_logged(update, "Updating prefix for this Wine", "prefix.log", Duration::from_secs(300))?;
    wine.cmd("wineserver", &cfg).arg("-w").status().ok();
    check_32bit(&cfg, &wine)?;

    ui::section("osu!");
    if let Some(dir) = opts.osu_dir {
        cfg.osu_dir = dir;
    }
    if cfg.exe().is_file() {
        ui::ok(format!("game found at {}", cfg.osu_dir.display()));
    } else {
        let in_place = match existing.as_ref().and_then(|e| e.osu_dir.as_ref()) {
            Some(dir) => {
                ui::info(format!("found an existing game folder at {}", dir.display()));
                choose(
                    "Which game folder should be used?",
                    &["The existing one, in place (songs, skins and scores stay where they are)", "A fresh download"],
                    opts.yes,
                )? == 0
            }
            None => false,
        };
        if in_place {
            cfg.osu_dir = existing.unwrap().osu_dir.unwrap();
            ui::ok(format!("using {}", cfg.osu_dir.display()));
        } else {
            std::fs::create_dir_all(&cfg.osu_dir)?;
            download(OSU_INSTALLER_URL, &cfg.exe(), "osu!")?;
            let head = std::fs::read(cfg.exe())?;
            if !head.starts_with(b"MZ") {
                std::fs::remove_file(cfg.exe()).ok();
                bail!("the osu! download is not a Windows executable (captive portal or network error?)");
            }
            ui::info("the game finishes downloading itself on first start");
        }
    }
    cfg.save()?;

    ui::section("Desktop integration");
    desktop::integrate()?;

    ui::section("System check");
    let problems = doctor::report(&cfg, true);
    if problems == 0 {
        ui::ok("nothing to fix");
    }

    eprintln!();
    ui::ok(format!("Installed. Start with `stdlinux run` or from your app menu. Preset: {}", cfg.preset));
    if system::is_nixos() && !wine.wrap.is_empty() {
        ui::hint("Running through steam-run. The flake package runs Wine natively, which lets GameMode see the game.");
    }
    Ok(())
}

pub fn uninstall(cfg: &Config, yes: bool) -> Result<()> {
    ui::banner("uninstall");
    let data = data_dir();
    let keeps_game = !cfg.osu_dir.starts_with(&data);
    ui::info(format!("removes Wine, the prefix and caches under {}", data.display()));
    if keeps_game {
        ui::info(format!("keeps your game folder {}", cfg.osu_dir.display()));
    } else {
        ui::warn(format!("your game folder {} is NOT removed; delete it yourself if you want", cfg.osu_dir.display()));
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("refusing to uninstall without --yes in a non-interactive shell");
        }
        if !dialoguer::Confirm::new().with_prompt("  Continue?").default(false).interact()? {
            return Ok(());
        }
    }
    Wine::locate().cmd("wineserver", cfg).arg("-k").status().ok();
    for sub in ["wine", "prefix", "cache", "shims", "logs"] {
        let p = data.join(sub);
        // Only ever the tool's own fixed subdirectories; the configured prefix is removed
        // just when it is the default one inside the data directory.
        if p.exists() && (sub != "prefix" || cfg.prefix == p) {
            std::fs::remove_dir_all(&p).with_context(|| format!("cannot remove {}", p.display()))?;
        }
    }
    desktop::remove()?;
    ui::ok("uninstalled");
    Ok(())
}
