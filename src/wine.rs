//! The pinned Wine build: download, verify, unpack, and build commands that run it.

use crate::config::{data_dir, Config};
use crate::ui;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// wine-osu: Wine Staging plus the osu! patch set (audio, input, timing, Wayland).
pub const WINE_TAG: &str = "wine-osu-staging-11.22-1";
const WINE_URL: &str = "https://github.com/NelloKudo/WineBuilder/releases/download/wine-osu-staging-11.22-1/wine-osu-winello-fonts-wow64-11.22-1-x86_64.tar.xz";
const WINE_SHA256: &str = "045c4de0863b5abb7b5a7bfef8e735dcd6dd8af0e3a2ed221f640206d18f288f";
/// Top-level directory inside the tarball.
const WINE_TOP: &str = "wine-osu";

pub struct Wine {
    /// Directory containing bin/wine.
    pub root: PathBuf,
    /// Command the Wine binaries are run through (empty, or e.g. ["steam-run"]).
    pub wrap: Vec<String>,
}

fn in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

pub fn have(name: &str) -> bool {
    in_path(name)
}

impl Wine {
    /// Where the build lives: a packaged one (`STDLINUX_WINE`, set by the Nix wrapper)
    /// or the one this tool downloads.
    pub fn locate() -> Wine {
        let packaged = std::env::var_os("STDLINUX_WINE").filter(|v| !v.is_empty());
        let root = match &packaged {
            Some(dir) => PathBuf::from(dir),
            None => data_dir().join("wine").join(WINE_TAG),
        };
        // The build is a generic-Linux binary. NixOS has no FHS library paths, so there it
        // runs inside a wrapper: the flake supplies its own, otherwise steam-run is used.
        let wrap = match std::env::var("STDLINUX_WINE_WRAP") {
            Ok(w) => w.split_whitespace().map(String::from).collect(),
            Err(_) if packaged.is_none() && Path::new("/etc/NIXOS").exists() && in_path("steam-run") => {
                vec!["steam-run".into()]
            }
            Err(_) => vec![],
        };
        Wine { root, wrap }
    }

    pub fn present(&self) -> bool {
        self.root.join("bin/wine").is_file()
    }

    pub fn bin(&self, name: &str) -> PathBuf {
        self.root.join("bin").join(name)
    }

    /// A command for one of Wine's binaries, with the prefix set and noise silenced.
    pub fn cmd(&self, bin: &str, cfg: &Config) -> Command {
        let mut argv: Vec<std::ffi::OsString> = self.wrap.iter().map(Into::into).collect();
        argv.push(self.bin(bin).into());
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        c.env("WINEPREFIX", &cfg.prefix);
        c.env("WINEDEBUG", std::env::var("WINEDEBUG").unwrap_or_else(|_| "-all".into()));
        c.env("WINEDLLOVERRIDES", "winemenubuilder.exe=");
        c.env("LC_ALL", "en_US.UTF-8").env("LANG", "en_US.UTF-8");
        c
    }

    pub fn version(&self, cfg: &Config) -> Result<String> {
        let out = self.cmd("wine", cfg).arg("--version").stderr(Stdio::null()).output()
            .context("cannot execute wine")?;
        if !out.status.success() {
            bail!("wine --version failed (missing system libraries?)");
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Shell shims for tools that want a single `wine` executable (winetricks).
    pub fn shim_dir(&self) -> Result<PathBuf> {
        let dir = data_dir().join("shims");
        std::fs::create_dir_all(&dir)?;
        for name in ["wine", "wineserver", "wineboot"] {
            let wrap = self.wrap.join(" ");
            let body = format!("#!/bin/sh\nexec {} \"{}\" \"$@\"\n", wrap, self.bin(name).display());
            let path = dir.join(name);
            std::fs::write(&path, body)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(dir)
    }

    /// Download and unpack the pinned build unless it is already there.
    pub fn ensure(&self) -> Result<()> {
        if self.present() {
            return Ok(());
        }
        if std::env::var_os("STDLINUX_WINE").is_some() {
            bail!("STDLINUX_WINE points at {} but there is no bin/wine in it", self.root.display());
        }
        let cache = data_dir().join("cache");
        std::fs::create_dir_all(&cache)?;
        let tarball = cache.join(format!("{WINE_TAG}.tar.xz"));
        if !tarball.is_file() || sha256_file(&tarball)? != WINE_SHA256 {
            download(WINE_URL, &tarball, "Wine")?;
            let got = sha256_file(&tarball)?;
            if got != WINE_SHA256 {
                std::fs::remove_file(&tarball).ok();
                bail!("Wine download is corrupt or was replaced upstream\n  expected {WINE_SHA256}\n  got      {got}");
            }
        }
        let staging = data_dir().join("wine").join(format!(".{WINE_TAG}.partial"));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        let sp = ui::spinner("Unpacking Wine");
        let status = Command::new("tar").arg("-xJf").arg(&tarball).arg("-C").arg(&staging).status()
            .context("cannot run tar (is tar/xz installed?)")?;
        sp.finish_and_clear();
        if !status.success() {
            bail!("tar failed to unpack {}", tarball.display());
        }
        std::fs::rename(staging.join(WINE_TOP), &self.root)?;
        std::fs::remove_dir_all(&staging).ok();
        ui::ok(format!("Wine unpacked ({WINE_TAG})"));
        Ok(())
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download to `dest` via a temporary file, with a progress bar.
pub fn download(url: &str, dest: &Path, what: &str) -> Result<()> {
    let resp = ureq::get(url).call().with_context(|| format!("download failed: {url}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let pb = ui::download_bar(total, what);
    let tmp = dest.with_extension("part");
    let mut out = std::fs::File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf).with_context(|| format!("connection lost while downloading {what}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        pb.inc(n as u64);
    }
    pb.finish_and_clear();
    out.sync_all()?;
    std::fs::rename(&tmp, dest)?;
    ui::ok(format!("{what} downloaded"));
    Ok(())
}
