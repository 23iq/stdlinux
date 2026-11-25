//! `stdlinux tune`: the few system settings that need root. Everything is written as
//! separate, clearly named files so it can be reviewed and removed.

use crate::{system, ui};
use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::process::Command;

/// (path, contents, why)
fn files(user: &str) -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "/etc/security/limits.d/99-stdlinux.conf",
            format!("{user} - rtprio 98\n{user} - nice -20\n{user} - memlock unlimited\n"),
            "lets Wine's audio thread and wineserver use realtime priority",
        ),
        (
            // Must sort before 73-seat-late.rules, which is what applies the uaccess tag.
            "/etc/udev/rules.d/70-stdlinux.rules",
            "KERNEL==\"cpu_dma_latency\", TAG+=\"uaccess\"\n".to_string(),
            "lets the launcher keep CPU cores out of deep sleep while the game runs",
        ),
        (
            "/etc/sysctl.d/99-stdlinux.conf",
            "kernel.split_lock_mitigate = 0\n".to_string(),
            "stops the kernel from throttling osu! when it takes a split lock",
        ),
        (
            "/etc/modules-load.d/stdlinux.conf",
            "ntsync\n".to_string(),
            "loads ntsync, the kernel implementation of Windows synchronisation",
        ),
    ]
}

pub fn tune(yes: bool, undo: bool) -> Result<()> {
    ui::banner(if undo { "tune --undo" } else { "tune" });
    if system::is_nixos() {
        ui::info("On NixOS these settings come from the flake's module:");
        ui::hint("imports = [ inputs.stdlinux.nixosModules.default ];\nprograms.stdlinux.enable = true;");
        return Ok(());
    }
    let user = std::env::var("USER").context("USER is not set")?;
    if user == "root" {
        bail!("run this as your normal user; it asks for sudo itself");
    }
    let files = files(&user);
    for (path, _, why) in &files {
        ui::info(*path);
        ui::hint(*why);
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("refusing to change system files without --yes in a non-interactive shell");
        }
        let q = if undo { "  Remove these files?" } else { "  Write these files (asks for sudo)?" };
        if !dialoguer::Confirm::new().with_prompt(q).default(true).interact()? {
            return Ok(());
        }
    }
    // One sudo call, so there is one password prompt.
    let mut script = String::from("set -e\n");
    for (path, body, _) in &files {
        if undo {
            script.push_str(&format!("rm -f '{path}'\n"));
        } else {
            script.push_str(&format!("mkdir -p \"$(dirname '{path}')\"\nprintf '%s' '{body}' > '{path}'\n"));
        }
    }
    script.push_str("udevadm control --reload 2>/dev/null || true\n");
    script.push_str("udevadm trigger --name-match=cpu_dma_latency 2>/dev/null || true\n");
    if !undo {
        script.push_str("sysctl -q --system 2>/dev/null || true\nmodprobe ntsync 2>/dev/null || true\n");
    }
    let status = Command::new("sudo").args(["sh", "-c", &script]).status().context("cannot run sudo")?;
    if !status.success() {
        bail!("sudo step failed; nothing else was changed");
    }
    if undo {
        ui::ok("system settings removed");
    } else {
        ui::ok("system settings written");
        ui::hint("Log out and back in for the realtime limits to apply, then check with `stdlinux doctor`.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_bodies_are_safe_inside_single_quotes() {
        for (path, body, _) in super::files("someone") {
            assert!(!path.contains('\''));
            assert!(!body.contains('\'') && !body.contains('%'));
        }
    }
}
