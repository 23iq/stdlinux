//! Read-only facts about the machine. Nothing here changes system state.

use std::path::Path;

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parses a kernel CPU list such as "0-15,18".
pub fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut out = vec![];
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    out.extend(a..=b);
                }
            }
            None => {
                if let Ok(n) = part.parse() {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// Performance cores of a hybrid Intel CPU, or None on a uniform CPU.
pub fn performance_cores() -> Option<Vec<usize>> {
    let p = parse_cpu_list(&read("/sys/devices/cpu_core/cpus")?);
    let e = parse_cpu_list(&read("/sys/devices/cpu_atom/cpus").unwrap_or_default());
    (!p.is_empty() && !e.is_empty()).then_some(p)
}

/// Restrict this process (and everything it starts) to the given CPUs.
pub fn pin_to(cpus: &[usize]) -> bool {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cpus {
            if c < libc::CPU_SETSIZE as usize {
                libc::CPU_SET(c, &mut set);
            }
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

pub fn nvidia() -> bool {
    Path::new("/proc/driver/nvidia/version").exists()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compositor {
    Niri,
    Hyprland,
    Sway,
    Kde,
    Gnome,
    Other,
    /// A plain X11 session.
    X11,
}

pub fn compositor() -> Compositor {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Compositor::X11;
    }
    if std::env::var_os("NIRI_SOCKET").is_some() {
        return Compositor::Niri;
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Compositor::Hyprland;
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return Compositor::Sway;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    if desktop.contains("niri") {
        Compositor::Niri
    } else if desktop.contains("kde") {
        Compositor::Kde
    } else if desktop.contains("gnome") {
        Compositor::Gnome
    } else {
        Compositor::Other
    }
}

/// Soft realtime-priority limit, as the shell's `ulimit -r` would print it.
pub fn rtprio_limit() -> u64 {
    unsafe {
        let mut lim: libc::rlimit = std::mem::zeroed();
        // The resource argument's type differs between glibc and musl; let it be inferred.
        if libc::getrlimit(libc::RLIMIT_RTPRIO as _, &mut lim) == 0 { lim.rlim_cur as u64 } else { 0 }
    }
}

/// PIDs of processes whose name (comm) is one of `names`.
pub fn pids_named(names: &[&str]) -> Vec<(u32, String)> {
    let mut out = vec![];
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if let Some(comm) = read(e.path().join("comm")) {
            // The kernel truncates comm to 15 bytes.
            if names.iter().any(|n| n.as_bytes().iter().take(15).eq(comm.as_bytes().iter())) {
                out.push((pid, comm));
            }
        }
    }
    out
}

/// Wine names the process after the executable, so this only matches a real client.
pub fn osu_running() -> bool {
    !pids_named(&["osu!.exe"]).is_empty()
}

pub struct Mouse {
    pub name: String,
    /// Highest report rate the device declares over USB, in Hz.
    pub declared_hz: u32,
}

/// USB mice and the polling rate their endpoints declare.
pub fn mice() -> Vec<Mouse> {
    let mut out: Vec<Mouse> = vec![];
    let Ok(dir) = std::fs::read_dir("/sys/bus/usb/devices") else { return out };
    for e in dir.flatten() {
        let iface = e.path();
        // HID (class 03) boot-protocol mouse (protocol 02).
        if read(iface.join("bInterfaceClass")).as_deref() != Some("03")
            || read(iface.join("bInterfaceProtocol")).as_deref() != Some("02")
        {
            continue;
        }
        let Some(dev) = iface.canonicalize().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else { continue };
        let speed: f64 = read(dev.join("speed")).and_then(|s| s.parse().ok()).unwrap_or(12.0);
        let name = read(dev.join("product")).unwrap_or_else(|| "USB mouse".into());
        let mut hz = 0u32;
        for ep in std::fs::read_dir(&iface).into_iter().flatten().flatten() {
            if !ep.file_name().to_string_lossy().starts_with("ep_8") {
                continue;
            }
            let Some(b) = read(ep.path().join("bInterval")).and_then(|s| u32::from_str_radix(&s, 16).ok()) else { continue };
            if b == 0 {
                continue;
            }
            // High-speed and faster: 2^(b-1) microframes of 125 us. Full/low speed: b milliseconds.
            let rate = if speed >= 480.0 { 8000 >> (b - 1).min(13) } else { 1000 / b };
            hz = hz.max(rate);
        }
        if hz > 0 {
            match out.iter_mut().find(|m| m.name == name) {
                Some(m) => m.declared_hz = m.declared_hz.max(hz),
                None => out.push(Mouse { name, declared_hz: hz }),
            }
        }
    }
    out
}

/// Deepest enabled CPU idle state: (name, wake-up latency in microseconds).
pub fn deepest_cstate() -> Option<(String, u32)> {
    let mut best: Option<(String, u32)> = None;
    for e in std::fs::read_dir("/sys/devices/system/cpu/cpu0/cpuidle").ok()?.flatten() {
        let p = e.path();
        if read(p.join("disable")).as_deref() == Some("1") {
            continue;
        }
        let lat: u32 = read(p.join("latency")).and_then(|s| s.parse().ok()).unwrap_or(0);
        if best.as_ref().is_none_or(|b| lat > b.1) {
            best = Some((read(p.join("name")).unwrap_or_default(), lat));
        }
    }
    best
}

pub const CPU_DMA_LATENCY: &str = "/dev/cpu_dma_latency";

/// Whether this user may limit CPU sleep depth (see `stdlinux tune`).
pub fn can_hold_cpu_latency() -> bool {
    std::fs::OpenOptions::new().write(true).open(CPU_DMA_LATENCY).is_ok()
}

/// Asks the kernel to keep wake-up latency under `max_us` on every core. The request
/// lasts exactly as long as the returned file stays open.
pub fn hold_cpu_latency(max_us: i32) -> Option<std::fs::File> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).open(CPU_DMA_LATENCY).ok()?;
    f.write_all(&max_us.to_ne_bytes()).ok()?;
    Some(f)
}

pub fn sysctl(key: &str) -> Option<String> {
    read(Path::new("/proc/sys").join(key.replace('.', "/")))
}

pub fn pipewire_running() -> bool {
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    Path::new(&rt).join("pipewire-0").exists()
}

pub fn is_nixos() -> bool {
    Path::new("/etc/NIXOS").exists()
}

#[cfg(test)]
mod tests {
    use super::parse_cpu_list;

    #[test]
    fn cpu_lists() {
        assert_eq!(parse_cpu_list("0-3"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("0-1,4,6-7\n"), vec![0, 1, 4, 6, 7]);
        assert!(parse_cpu_list("").is_empty());
    }
}
