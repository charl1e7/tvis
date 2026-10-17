//! User settings and their persistence.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tvis_core::Target;
use tvis_core::alert::AlertRule;
use tvis_core::units::ByteBase;

/// Choices offered by the UI; the indices are what the views persist.
pub const INTERVALS_MS: [u64; 6] = [250, 500, 1000, 2000, 5000, 10_000];
pub const RETENTIONS_SECS: [u64; 5] = [300, 900, 3600, 21_600, 86_400];
pub const WINDOWS_SECS: [f64; 5] = [30.0, 60.0, 300.0, 900.0, 3600.0];

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn index(self) -> i32 {
        self as i32
    }

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Dark,
            2 => Self::Light,
            _ => Self::System,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: ThemeMode,
    pub accent: usize,
    pub interval_ms: u64,
    pub retention_secs: u64,
    pub cpu_normalized: bool,
    pub byte_base: ByteBase,
    pub smoothing: u8,
    pub line_width: u8,
    pub chart_fill: bool,
    pub chart_grid: bool,
    pub window_secs: f64,
    pub watchlist: Vec<Target>,
    pub alerts: Vec<AlertRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemeMode::System,
            accent: 0,
            interval_ms: 1000,
            retention_secs: 3600,
            cpu_normalized: false,
            byte_base: ByteBase::Binary,
            smoothing: 0,
            line_width: 1,
            chart_fill: true,
            chart_grid: true,
            window_secs: 300.0,
            watchlist: Vec::new(),
            alerts: Vec::new(),
        }
    }
}

fn nearest_index<T: Copy + Into<f64>>(table: &[T], value: f64) -> usize {
    table
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let (da, db) = ((*a.1).into() - value, (*b.1).into() - value);
            da.abs().total_cmp(&db.abs())
        })
        .map_or(0, |(i, _)| i)
}

impl Settings {
    pub fn interval_index(&self) -> usize {
        nearest_index(&INTERVALS_MS.map(|v| v as f64), self.interval_ms as f64)
    }

    pub fn retention_index(&self) -> usize {
        nearest_index(
            &RETENTIONS_SECS.map(|v| v as f64),
            self.retention_secs as f64,
        )
    }

    pub fn window_index(&self) -> usize {
        nearest_index(&WINDOWS_SECS, self.window_secs)
    }

    /// Clamps values that may have been hand-edited into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self.interval_ms = INTERVALS_MS[self.interval_index()];
        self.retention_secs = RETENTIONS_SECS[self.retention_index()];
        self.window_secs = WINDOWS_SECS[self.window_index()];
        self.accent = self.accent.min(5);
        self.smoothing = self.smoothing.min(5);
        self.line_width = self.line_width.min(2);
        // PIDs do not survive a restart, so only name targets are remembered.
        self.watchlist.retain(|t| matches!(t, Target::Name(_)));
        self
    }
}

/// Reads and writes `settings.toml` inside the configuration directory.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    /// Platform config dir, overridable with `TVIS_CONFIG_DIR`.
    pub fn default_location() -> Self {
        let dir = std::env::var_os("TVIS_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                ProjectDirs::from("dev", "charl1e7", "tvis").map(|d| d.config_dir().to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from("."));
        Self { dir }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self) -> PathBuf {
        self.dir.join("settings.toml")
    }

    /// Loads settings; a missing file yields defaults, a corrupt one is
    /// moved aside to `settings.toml.bak` and defaults are used.
    pub fn load(&self) -> Settings {
        let text = match fs::read_to_string(self.file()) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Settings::default(),
            Err(e) => {
                tracing::warn!("cannot read settings: {e}");
                return Settings::default();
            }
        };
        match toml::from_str::<Settings>(&text) {
            Ok(s) => s.sanitized(),
            Err(e) => {
                tracing::warn!("settings are corrupt ({e}); starting from defaults");
                let _ = fs::rename(self.file(), self.dir.join("settings.toml.bak"));
                Settings::default()
            }
        }
    }

    /// Writes atomically (temp file + rename) so a crash never truncates the file.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let text = toml::to_string_pretty(settings).map_err(io::Error::other)?;
        let tmp = self.dir.join("settings.toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, self.file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvis_core::alert::Comparison;

    #[test]
    fn roundtrip_preserves_everything() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path());
        let s = Settings {
            theme: ThemeMode::Dark,
            accent: 3,
            interval_ms: 500,
            watchlist: vec![Target::Name("java".into())],
            alerts: vec![AlertRule {
                id: 7,
                name: "hot".into(),
                target: None,
                metric: "cpu".into(),
                comparison: Comparison::Above,
                threshold: 90.0,
                for_secs: 10.0,
                enabled: true,
            }],
            ..Settings::default()
        };
        store.save(&s).unwrap();
        assert_eq!(store.load(), s);
    }

    #[test]
    fn missing_file_and_missing_fields_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path());
        assert_eq!(store.load(), Settings::default());
        fs::write(dir.path().join("settings.toml"), "accent = 2\n").unwrap();
        let s = store.load();
        assert_eq!(s.accent, 2);
        assert_eq!(s.interval_ms, 1000);
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.toml"), "this is = = not toml").unwrap();
        let store = SettingsStore::at(dir.path());
        assert_eq!(store.load(), Settings::default());
        assert!(dir.path().join("settings.toml.bak").exists());
    }

    #[test]
    fn sanitizing_snaps_values_and_drops_pid_targets() {
        let s = Settings {
            interval_ms: 600,
            accent: 99,
            window_secs: 290.0,
            watchlist: vec![Target::Pid(5), Target::Name("x".into())],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.interval_ms, 500);
        assert_eq!(s.accent, 5);
        assert_eq!(s.window_secs, 300.0);
        assert_eq!(s.watchlist, vec![Target::Name("x".into())]);
    }
}
