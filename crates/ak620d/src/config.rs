//! Versioned daemon configuration stored below the XDG config directory.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use ak620_core::TemperatureUnit;
use serde::{Deserialize, Serialize};

/// Current on-disk configuration schema version.
pub const CONFIG_VERSION: u32 = 1;
/// Smallest accepted display refresh interval.
pub const MIN_UPDATE_INTERVAL_MS: u64 = 250;
/// Largest accepted display refresh interval.
pub const MAX_UPDATE_INTERVAL_MS: u64 = 10_000;
const DEFAULT_UPDATE_INTERVAL_MS: u64 = 1_000;

/// User-selectable display temperature unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigTemperatureUnit {
    /// Degrees Celsius.
    Celsius,
    /// Degrees Fahrenheit.
    Fahrenheit,
}

impl From<ConfigTemperatureUnit> for TemperatureUnit {
    fn from(unit: ConfigTemperatureUnit) -> Self {
        match unit {
            ConfigTemperatureUnit::Celsius => Self::Celsius,
            ConfigTemperatureUnit::Fahrenheit => Self::Fahrenheit,
        }
    }
}

/// Validated version-one settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: u32,
    update_interval_ms: u64,
    temperature_unit: ConfigTemperatureUnit,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            update_interval_ms: DEFAULT_UPDATE_INTERVAL_MS,
            temperature_unit: ConfigTemperatureUnit::Celsius,
        }
    }
}

impl Config {
    /// Parses and validates one TOML document.
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Loads a config file, returning defaults only when the file does not exist.
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(contents) => Self::parse(&contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Persists this validated config using a same-directory atomic rename.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let parent = path.parent().ok_or_else(|| ConfigError::InvalidPath {
            detail: format!("{} has no parent directory", path.display()),
        })?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            path: parent.to_owned(),
            source,
        })?;
        let contents = toml::to_string_pretty(self)?;
        let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
        fs::write(&temporary, contents).map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| ConfigError::Io {
            path: path.to_owned(),
            source,
        })
    }

    /// Returns the version-one refresh period in milliseconds.
    pub const fn update_interval_ms(&self) -> u64 {
        self.update_interval_ms
    }

    /// Returns the selected temperature unit.
    pub const fn temperature_unit(&self) -> ConfigTemperatureUnit {
        self.temperature_unit
    }

    /// Validates and changes the refresh period.
    pub fn set_update_interval_ms(&mut self, value: u64) -> Result<(), ConfigError> {
        validate_interval(value)?;
        self.update_interval_ms = value;
        Ok(())
    }

    /// Changes the unit stored in subsequent reports.
    pub const fn set_temperature_unit(&mut self, unit: ConfigTemperatureUnit) {
        self.temperature_unit = unit;
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: self.version,
            });
        }
        validate_interval(self.update_interval_ms)
    }
}

/// Returns the system-owned daemon state location managed by systemd.
pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    Ok(PathBuf::from("/var/lib/ak620-linux/config.toml"))
}

fn validate_interval(value: u64) -> Result<(), ConfigError> {
    if !(MIN_UPDATE_INTERVAL_MS..=MAX_UPDATE_INTERVAL_MS).contains(&value) {
        return Err(ConfigError::InvalidInterval {
            milliseconds: value,
        });
    }
    Ok(())
}

/// Invalid configuration, path, or storage operation.
#[derive(Debug)]
pub enum ConfigError {
    /// TOML syntax or type mismatch.
    Parse(toml::de::Error),
    /// TOML serialization failed.
    Serialize(toml::ser::Error),
    /// Configuration schema version is not supported.
    UnsupportedVersion { found: u32 },
    /// Update interval falls outside safe bounds.
    InvalidInterval { milliseconds: u64 },
    /// An XDG or target path is unusable.
    InvalidPath { detail: String },
    /// A file operation failed.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "invalid configuration TOML: {error}"),
            Self::Serialize(error) => {
                write!(formatter, "could not serialize configuration: {error}")
            }
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "unsupported configuration version {found}; expected {CONFIG_VERSION}"
            ),
            Self::InvalidInterval { milliseconds } => write!(
                formatter,
                "update interval {milliseconds} ms is outside {MIN_UPDATE_INTERVAL_MS}..={MAX_UPDATE_INTERVAL_MS} ms"
            ),
            Self::InvalidPath { detail } => {
                write!(formatter, "invalid configuration path: {detail}")
            }
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Serialize(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::UnsupportedVersion { .. }
            | Self::InvalidInterval { .. }
            | Self::InvalidPath { .. } => None,
        }
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(error: toml::de::Error) -> Self {
        Self::Parse(error)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Serialize(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONFIG_VERSION, Config, ConfigError, ConfigTemperatureUnit, MAX_UPDATE_INTERVAL_MS,
        MIN_UPDATE_INTERVAL_MS,
    };
    use crate::metrics::test_support::FixtureDir;

    #[test]
    fn parses_a_complete_versioned_document() {
        let config = Config::parse(
            "version = 1\nupdate_interval_ms = 750\ntemperature_unit = \"fahrenheit\"\n",
        )
        .unwrap();

        assert_eq!(config.update_interval_ms(), 750);
        assert_eq!(config.temperature_unit(), ConfigTemperatureUnit::Fahrenheit);
    }

    #[test]
    fn rejects_unknown_versions_fields_and_unsafe_intervals() {
        assert!(matches!(
            Config::parse(
                "version = 2\nupdate_interval_ms = 1000\ntemperature_unit = \"celsius\"\n"
            ),
            Err(ConfigError::UnsupportedVersion { found: 2 })
        ));
        assert!(matches!(
            Config::parse("version = 1\nupdate_interval_ms = 10\ntemperature_unit = \"celsius\"\n"),
            Err(ConfigError::InvalidInterval { milliseconds: 10 })
        ));
        assert!(Config::parse(
            "version = 1\nupdate_interval_ms = 1000\ntemperature_unit = \"celsius\"\nextra = true\n"
        )
        .is_err());
    }

    #[test]
    fn accepts_interval_boundaries() {
        let mut config = Config::default();
        config
            .set_update_interval_ms(MIN_UPDATE_INTERVAL_MS)
            .unwrap();
        config
            .set_update_interval_ms(MAX_UPDATE_INTERVAL_MS)
            .unwrap();
    }

    #[test]
    fn missing_file_uses_defaults_and_save_round_trips() {
        let fixture = FixtureDir::new("config");
        let path = fixture.path().join("nested/config.toml");
        let mut config = Config::load_or_default(&path).unwrap();
        assert_eq!(config.update_interval_ms(), 1_000);
        config.set_temperature_unit(ConfigTemperatureUnit::Fahrenheit);
        config.save(&path).unwrap();

        assert_eq!(Config::load_or_default(&path).unwrap(), config);
        assert_eq!(
            Config::load_or_default(&path).unwrap().version,
            CONFIG_VERSION
        );
    }
}
