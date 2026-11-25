//! Paths, presets and the user config file.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("stdlinux")
}

pub fn share_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("stdlinux/config.toml")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    /// XWayland/X11 when an X server is reachable, native Wayland otherwise.
    Auto,
    X11,
    Wayland,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Audio {
    Pipewire,
    Pulse,
}

/// Everything a preset decides. Any field can be overridden in `[overrides]`.
#[derive(Clone, Debug)]
pub struct Settings {
    pub display: Display,
    pub audio: Audio,
    /// Audio buffer in frames at 48 kHz (64 = 1.3 ms).
    pub quantum: u32,
    /// Keep the game off the efficiency cores on hybrid CPUs.
    pub pin_pcores: bool,
    pub nvidia_low_latency: bool,
    pub gamemode: bool,
    /// Realtime priority for wineserver (needs an rtprio limit > 0).
    pub realtime: bool,
    /// Ask the compositor to really fullscreen the game window.
    pub fullscreen: bool,
    /// Keep CPU cores out of deep sleep states while the game runs.
    pub cpu_latency: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub display: Option<Display>,
    pub audio: Option<Audio>,
    pub quantum: Option<u32>,
    pub pin_pcores: Option<bool>,
    pub nvidia_low_latency: Option<bool>,
    pub gamemode: Option<bool>,
    pub realtime: Option<bool>,
    pub fullscreen: Option<bool>,
    pub cpu_latency: Option<bool>,
}

pub struct Preset {
    pub name: &'static str,
    pub about: &'static str,
    pub settings: Settings,
}

pub fn presets() -> Vec<Preset> {
    let base = Settings {
        display: Display::Auto,
        audio: Audio::Pipewire,
        quantum: 64,
        pin_pcores: true,
        nvidia_low_latency: true,
        gamemode: true,
        realtime: false,
        fullscreen: true,
        cpu_latency: true,
    };
    vec![
        Preset {
            name: "latency",
            about: "default: PipeWire driver at 64 frames, P-cores only, NVIDIA low-latency, XWayland",
            settings: base.clone(),
        },
        Preset {
            name: "ultra",
            about: "32-frame audio and realtime wineserver; needs rtprio limits (see `stdlinux doctor`)",
            settings: Settings { quantum: 32, realtime: true, ..base.clone() },
        },
        Preset {
            name: "wayland",
            about: "like latency, but the native Wayland driver (tearing where the compositor allows it)",
            settings: Settings { display: Display::Wayland, ..base.clone() },
        },
        Preset {
            name: "balanced",
            about: "128-frame audio, no core pinning; for weak CPUs or crackling audio",
            settings: Settings { quantum: 128, pin_pcores: false, ..base.clone() },
        },
        Preset {
            name: "compat",
            about: "no tuning at all: PulseAudio driver, default buffers; for troubleshooting",
            settings: Settings {
                audio: Audio::Pulse,
                quantum: 1024,
                pin_pcores: false,
                nvidia_low_latency: false,
                gamemode: false,
                fullscreen: false,
                cpu_latency: false,
                ..base
            },
        },
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub preset: String,
    pub prefix: PathBuf,
    /// Folder that holds osu!.exe, Songs, Skins. Lives outside the prefix so it survives rebuilds.
    pub osu_dir: PathBuf,
    #[serde(default)]
    pub overrides: Overrides,
    /// Extra environment for the game, applied last.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            preset: "latency".into(),
            prefix: data_dir().join("prefix"),
            osu_dir: data_dir().join("osu"),
            overrides: Overrides::default(),
            env: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Config> {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("bad config {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("cannot write {}", path.display()))
    }

    pub fn exe(&self) -> PathBuf {
        self.osu_dir.join("osu!.exe")
    }

    pub fn installed(&self) -> bool {
        self.exe().is_file() && self.prefix.join("system.reg").is_file()
    }

    pub fn settings(&self) -> Result<Settings> {
        let mut s = presets()
            .into_iter()
            .find(|p| p.name == self.preset)
            .with_context(|| format!("unknown preset '{}' (see `stdlinux preset`)", self.preset))?
            .settings;
        let o = &self.overrides;
        if let Some(v) = o.display { s.display = v }
        if let Some(v) = o.audio { s.audio = v }
        if let Some(v) = o.quantum { s.quantum = v }
        if let Some(v) = o.pin_pcores { s.pin_pcores = v }
        if let Some(v) = o.nvidia_low_latency { s.nvidia_low_latency = v }
        if let Some(v) = o.gamemode { s.gamemode = v }
        if let Some(v) = o.realtime { s.realtime = v }
        if let Some(v) = o.fullscreen { s.fullscreen = v }
        if let Some(v) = o.cpu_latency { s.cpu_latency = v }
        Ok(s)
    }
}
