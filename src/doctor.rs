//! `stdlinux doctor`: what on this machine costs latency or smoothness, and how to fix it.

use crate::config::Config;
use crate::system::{self, Compositor};
use crate::ui;
use crate::wine::{have, Wine};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

const RECORDERS: &[&str] = &["gpu-screen-recorder", "gsr-kms-server", "obs", "wf-recorder", "wl-screenrec"];

fn tune_hint() -> String {
    if system::is_nixos() {
        "Fix: enable the NixOS module from the flake (programs.stdlinux.enable = true).".into()
    } else {
        "Fix: run `stdlinux tune` (asks for sudo once), then log out and back in.".into()
    }
}

/// Prints the checks. With `brief`, only problems are shown. Returns the number of problems.
pub fn report(cfg: &Config, brief: bool) -> usize {
    let mut problems = 0;
    let good = |msg: String| {
        if !brief {
            ui::ok(msg)
        }
    };
    let mut bad = |msg: String, fix: &str| {
        problems += 1;
        ui::warn(msg);
        ui::hint(fix);
    };

    // Input
    for m in system::mice() {
        if m.declared_hz > 1000 {
            bad(
                format!("{} can report at {} Hz", m.name, m.declared_hz),
                "Polling above 1000 Hz makes osu! stutter while the cursor moves, even at high fps.\nSet 1000 Hz in the mouse software, then verify with `stdlinux doctor --mouse`.",
            );
        } else {
            good(format!("{}: {} Hz polling", m.name, m.declared_hz));
        }
    }

    // Scheduling
    if system::rtprio_limit() == 0 {
        bad("realtime priority is not allowed for your user (rtprio limit is 0)".into(), &tune_hint());
    } else {
        good("realtime priority allowed".into());
    }
    match system::performance_cores() {
        Some(p) => good(format!("hybrid CPU: the game is kept on {} P-core threads", p.len())),
        None => good("uniform CPU: no core pinning needed".into()),
    }
    if let Some((name, lat)) = system::deepest_cstate() {
        if lat <= 150 {
            good(format!("CPU idle states are shallow (deepest {name}, {lat} us)"));
        } else if system::can_hold_cpu_latency() {
            good(format!("deep CPU sleep ({name}) is blocked while the game runs"));
        } else {
            bad(
                format!("deep CPU sleep state {name} is enabled (up to {:.2} ms to wake)", lat as f64 / 1000.0),
                &format!("Idle cores wake slowly, which adds jitter to input and audio.\n{}", tune_hint()),
            );
        }
    }
    if system::sysctl("kernel.split_lock_mitigate").as_deref() == Some("1") {
        bad(
            "split-lock mitigation is on".into(),
            &format!("The kernel slows a process down when it takes a split lock, and osu! does.\n{}", tune_hint()),
        );
    }
    if have("gamemoderun") {
        good("GameMode available".into());
    } else {
        bad("GameMode is not installed".into(), "Install the `gamemode` package; the launcher uses it automatically.");
    }
    if std::path::Path::new("/dev/ntsync").exists() {
        good("ntsync available".into());
    } else {
        bad("ntsync is not available".into(), "Needs Linux 6.14+ with the ntsync module loaded: sudo modprobe ntsync");
    }

    // Audio
    if system::pipewire_running() {
        good("PipeWire running".into());
    } else {
        bad("PipeWire is not running".into(), "The default presets use the PipeWire driver. Use `stdlinux preset compat` for PulseAudio.");
    }

    // Display
    match system::compositor() {
        Compositor::Niri => {
            bad(
                "niri does not support tearing".into(),
                "Every frame waits for the next refresh (up to one refresh interval of extra latency).\nNothing to fix in stdlinux; the game window is fullscreened automatically for direct scanout.",
            );
        }
        Compositor::X11 => good("X11 session: no compositor in the way if compositing is off".into()),
        c => good(format!("{c:?}: allow tearing for osu! in the compositor settings for the lowest latency")),
    }
    let recorders = system::pids_named(RECORDERS);
    if !recorders.is_empty() {
        let mut names: Vec<&str> = recorders.iter().map(|r| r.1.as_str()).collect();
        names.dedup();
        bad(
            format!("screen recorder running: {}", names.join(", ")),
            "Continuous capture can cause uneven frame pacing. Stop it while playing if you see hitches.",
        );
    }

    // Install state
    if !brief {
        let wine = Wine::locate();
        match wine.version(cfg) {
            Ok(v) => ui::ok(format!("Wine: {v}")),
            Err(_) => {
                problems += 1;
                ui::fail("Wine is not installed (run `stdlinux install`)");
            }
        }
        if cfg.installed() {
            ui::ok(format!("osu! at {}", cfg.osu_dir.display()));
        } else {
            problems += 1;
            ui::fail("osu! is not installed (run `stdlinux install`)");
        }
    }
    problems
}

/// Measures the real report rate of every pointing device while the user moves the mouse.
pub fn mouse_test(secs: u64) -> anyhow::Result<()> {
    ui::banner("mouse polling test");
    let mut devices = vec![];
    for e in std::fs::read_dir("/dev/input")?.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        if !fname.starts_with("event") {
            continue;
        }
        let sys = format!("/sys/class/input/{fname}/device");
        let rel = std::fs::read_to_string(format!("{sys}/capabilities/rel")).unwrap_or_default();
        if matches!(rel.trim(), "" | "0") {
            continue;
        }
        let name = std::fs::read_to_string(format!("{sys}/name")).unwrap_or_default().trim().to_string();
        if let Ok(f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(e.path()) {
            devices.push((name, f, BTreeMap::<u64, u32>::new()));
        }
    }
    if devices.is_empty() {
        anyhow::bail!("cannot read /dev/input (add yourself to the `input` group and log in again)");
    }
    ui::info(format!("move the mouse in fast circles for {secs} seconds..."));
    let start = Instant::now();
    let mut buf = [0u8; 24 * 256];
    while start.elapsed() < Duration::from_secs(secs) {
        let mut idle = true;
        for (_, f, buckets) in devices.iter_mut() {
            while let Ok(n) = f.read(&mut buf) {
                if n < 24 {
                    break;
                }
                idle = false;
                for ev in buf[..n].as_chunks::<24>().0 {
                    // struct input_event: timeval (16 bytes), type u16, code u16, value i32
                    let ty = u16::from_ne_bytes([ev[16], ev[17]]);
                    let code = u16::from_ne_bytes([ev[18], ev[19]]);
                    if ty == 0 && code == 0 {
                        // SYN_REPORT closes one hardware report; count per 100 ms window.
                        *buckets.entry(start.elapsed().as_millis() as u64 / 100).or_default() += 1;
                    }
                }
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let mut any = false;
    for (name, _, buckets) in &devices {
        let Some(peak) = buckets.values().max() else { continue };
        let hz = peak * 10;
        if hz < 50 {
            continue;
        }
        any = true;
        if hz > 1100 {
            ui::warn(format!("{name}: {hz} reports/s"));
            ui::hint("Too fast for osu! stable: set the mouse to 1000 Hz.");
        } else {
            ui::ok(format!("{name}: {hz} reports/s"));
        }
    }
    if !any {
        ui::warn("no movement seen");
    }
    Ok(())
}
