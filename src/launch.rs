//! Starting the game with the tuned environment.

use crate::config::{data_dir, Audio, Config, Display, Settings};
use crate::system::{self, Compositor};
use crate::ui;
use crate::wine::{have, Wine};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What the preset resolves to on this machine.
pub struct Plan {
    pub display: Display,
    pub pin: Option<Vec<usize>>,
    pub nvidia: bool,
    pub gamemode: bool,
    /// (name, value); None removes the variable.
    pub env: Vec<(String, Option<String>)>,
}

pub fn plan(s: &Settings) -> Plan {
    let x_available = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    let wl_available = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let display = match s.display {
        Display::Wayland if wl_available => Display::Wayland,
        Display::X11 if x_available => Display::X11,
        _ if x_available => Display::X11,
        _ => Display::Wayland,
    };
    let nvidia = s.nvidia_low_latency && system::nvidia();
    let mut env: Vec<(String, Option<String>)> = vec![];
    let mut set = |k: &str, v: &str| env.push((k.to_string(), Some(v.to_string())));

    set("WINENTSYNC", "1");
    set("WINEFSYNC", "1");
    set("WINEESYNC", "1");
    set("WINE_DISABLE_FULLSCREEN_HACK", "1");
    // The game paces itself; driver-side vsync only adds a queue.
    set("vblank_mode", "0");
    set("__GL_SYNC_TO_VBLANK", "0");

    match s.audio {
        Audio::Pipewire => {
            set("WINE_AUDIO_DRIVER", "pipewire");
            set("WINE_AUDIO_QUANTUM", &s.quantum.to_string());
        }
        Audio::Pulse => {
            let ms = (s.quantum as u64 * 1000 / 48000).max(1);
            set("PULSE_LATENCY_MSEC", &ms.to_string());
            set("PIPEWIRE_LATENCY", &format!("{}/48000", s.quantum));
        }
    }
    if nvidia {
        set("__GL_MaxFramesAllowed", "1");
        set("__GL_THREADED_OPTIMIZATIONS", "0");
        set("__GL_GSYNC_ALLOWED", "0");
        set("__GL_VRR_ALLOWED", "0");
    }
    if s.realtime {
        set("STAGING_RT_PRIORITY_SERVER", "1");
    }
    // Wine picks its driver from what is reachable, so hiding the other socket selects one
    // regardless of what the prefix registry prefers.
    match display {
        Display::Wayland => env.push(("DISPLAY".into(), None)),
        _ => env.push(("WAYLAND_DISPLAY".into(), None)),
    }
    Plan {
        display,
        pin: s.pin_pcores.then(system::performance_cores).flatten(),
        nvidia,
        gamemode: s.gamemode && have("gamemoderun"),
        env,
    }
}

/// osu! downloads updates into `_pending` and installs them on the next start by
/// overwriting files it has already mapped, which Wine refuses. Nothing holds those
/// files before the client starts, so the swap is done here.
fn apply_pending_update(osu_dir: &Path) -> Result<()> {
    let pending = osu_dir.join("_pending");
    let Ok(entries) = std::fs::read_dir(&pending) else { return Ok(()) };
    let mut moved = 0;
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let dest = osu_dir.join(e.file_name());
            std::fs::rename(e.path(), &dest)
                .or_else(|_| std::fs::copy(e.path(), &dest).and_then(|_| std::fs::remove_file(e.path())))
                .with_context(|| format!("cannot install pending update file {:?}", e.file_name()))?;
            moved += 1;
        }
    }
    if moved > 0 {
        ui::info(format!("installed pending osu! update ({moved} files)"));
    }
    std::fs::remove_dir(&pending).ok();
    Ok(())
}

/// A Linux path as the game sees it (Wine maps Z: to /).
pub fn windows_path(p: &Path) -> String {
    format!("Z:{}", p.to_string_lossy().replace('/', "\\"))
}

/// The command that starts osu!, with everything applied. `args` go to the game.
pub fn game_command(cfg: &Config, wine: &Wine, args: &[String]) -> Result<(Command, Plan)> {
    let s = cfg.settings()?;
    let p = plan(&s);
    let inner = wine.cmd("wine", cfg);
    let mut argv: Vec<std::ffi::OsString> = vec![];
    if p.gamemode {
        argv.push("gamemoderun".into());
    }
    argv.push(inner.get_program().to_owned());
    argv.extend(inner.get_args().map(|a| a.to_owned()));
    argv.push(cfg.exe().into());
    argv.extend(args.iter().map(Into::into));

    let mut c = Command::new(&argv[0]);
    c.args(&argv[1..]);
    for (k, v) in inner.get_envs() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    for (k, v) in &p.env {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    for (k, v) in &cfg.env {
        c.env(k, v);
    }
    c.current_dir(&cfg.osu_dir);
    Ok((c, p))
}

pub fn describe(cfg: &Config, p: &Plan, s: &Settings) -> String {
    let display = if p.display == Display::Wayland { "Wayland" } else { "X11" };
    let audio = match s.audio {
        Audio::Pipewire => format!("PipeWire {} frames ({:.1} ms)", s.quantum, s.quantum as f64 / 48.0),
        Audio::Pulse => format!("Pulse {} frames", s.quantum),
    };
    let mut parts = vec![format!("preset {}", cfg.preset), display.to_string(), audio];
    if let Some(cpus) = &p.pin {
        parts.push(format!("{} P-core threads", cpus.len()));
    }
    if p.nvidia {
        parts.push("NVIDIA low-latency".into());
    }
    if p.gamemode {
        parts.push("GameMode".into());
    }
    if s.realtime {
        parts.push("realtime".into());
    }
    if s.cpu_latency && system::can_hold_cpu_latency() {
        parts.push("no deep CPU sleep".into());
    }
    parts.join(" · ")
}

pub fn run(cfg: &Config, args: &[String]) -> Result<()> {
    if !cfg.installed() {
        bail!("osu! is not installed yet. Run `stdlinux install`.");
    }
    let wine = Wine::locate();
    if !wine.present() {
        bail!("Wine is missing at {}. Run `stdlinux install`.", wine.root.display());
    }
    if system::osu_running() {
        bail!("osu! is already running.");
    }
    apply_pending_update(&cfg.osu_dir)?;

    let s = cfg.settings()?;
    let (mut cmd, p) = game_command(cfg, &wine, args)?;
    ui::info(describe(cfg, &p, &s));
    if s.realtime && system::rtprio_limit() == 0 {
        ui::warn("realtime is on but your rtprio limit is 0, so it has no effect (see `stdlinux doctor`)");
    }
    if let Some(cpus) = &p.pin {
        if !system::pin_to(cpus) {
            ui::warn("could not pin to the performance cores");
        }
    }

    // 10 us still allows the light C1 states but rules out the deep ones, which take
    // hundreds of microseconds to leave. Dropped when this function returns.
    let _cpu_latency = if s.cpu_latency { system::hold_cpu_latency(10) } else { None };

    let logs = data_dir().join("logs");
    std::fs::create_dir_all(&logs)?;
    let log = std::fs::File::create(logs.join("wine.log"))?;
    cmd.stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    let mut child = cmd.spawn().context("cannot start Wine")?;

    if s.fullscreen {
        std::thread::spawn(|| fullscreen_watch(Duration::from_secs(90)));
    }
    let status = child.wait()?;
    if !status.success() {
        ui::warn(format!("Wine exited with {status}; log: {}", logs.join("wine.log").display()));
    }
    Ok(())
}

/// Some compositors keep an X11 "fullscreen" window as an ordinary tile, which forces
/// compositing on every frame. Ask them to fullscreen it for real.
fn fullscreen_watch(for_how_long: Duration) {
    if system::compositor() != Compositor::Niri {
        return;
    }
    let start = Instant::now();
    let mut done: Vec<u64> = vec![];
    while start.elapsed() < for_how_long {
        std::thread::sleep(Duration::from_millis(700));
        let Some(windows) = niri_json(&["windows"]) else { continue };
        let outputs = niri_json(&["outputs"]);
        for w in windows.as_array().into_iter().flatten() {
            if w["app_id"].as_str() != Some("osu!.exe") {
                continue;
            }
            let Some(id) = w["id"].as_u64() else { continue };
            if done.contains(&id) {
                continue;
            }
            let tile_w = w["layout"]["tile_size"][0].as_f64().unwrap_or(0.0);
            if tile_w < 1.0 {
                continue;
            }
            let covers_output = outputs
                .as_ref()
                .and_then(|o| o.as_object())
                .map(|o| o.values().any(|out| out["logical"]["width"].as_f64() == Some(tile_w)))
                .unwrap_or(false);
            if !covers_output {
                Command::new("niri")
                    .args(["msg", "action", "fullscreen-window", "--id", &id.to_string()])
                    .status()
                    .ok();
            }
            done.push(id);
        }
    }
}

fn niri_json(what: &[&str]) -> Option<serde_json::Value> {
    let out = Command::new("niri").arg("msg").arg("--json").args(what).stderr(Stdio::null()).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

pub fn kill(cfg: &Config) -> Result<()> {
    let wine = Wine::locate();
    wine.cmd("wineserver", cfg).arg("-k").status().ok();
    ui::ok("stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::presets;

    fn env_of(p: &Plan, key: &str) -> Option<Option<String>> {
        p.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn windows_paths() {
        assert_eq!(windows_path(Path::new("/home/a/b c.osz")), "Z:\\home\\a\\b c.osz");
    }

    #[test]
    fn pipewire_preset_sets_quantum() {
        let s = presets().into_iter().find(|p| p.name == "ultra").unwrap().settings;
        let p = plan(&s);
        assert_eq!(env_of(&p, "WINE_AUDIO_QUANTUM"), Some(Some("32".into())));
        assert_eq!(env_of(&p, "STAGING_RT_PRIORITY_SERVER"), Some(Some("1".into())));
    }

    #[test]
    fn compat_preset_leaves_driver_alone() {
        let s = presets().into_iter().find(|p| p.name == "compat").unwrap().settings;
        let p = plan(&s);
        assert_eq!(env_of(&p, "WINE_AUDIO_DRIVER"), None);
        assert_eq!(env_of(&p, "__GL_MaxFramesAllowed"), None);
        assert!(p.pin.is_none() && !p.gamemode);
    }
}
