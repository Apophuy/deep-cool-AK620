//! Per-user presentation preferences. They never affect HID access or daemon settings.

use std::{env, fs, path::PathBuf};

use eframe::egui;

use crate::model::{DaemonSnapshot, TemperatureChoice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Language {
    English,
    Russian,
}

impl Language {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }
    pub(crate) fn text(self, english: &'static str) -> &'static str {
        if self == Self::English {
            return english;
        }
        match english {
            "Connected" => "Подключено",
            "Disconnected" => "Отключено",
            "Temperature" => "Температура",
            "CPU utilization" => "Загрузка ЦП",
            "CPU package power" => "Мощность пакета ЦП",
            "Highest core frequency" => "Максимальная частота ядра",
            "Display settings" => "Настройки дисплея",
            "Choose how values are shown on the cooler." => {
                "Выберите, как значения отображаются на кулере."
            }
            "Temperature unit" => "Единица температуры",
            "Refresh interval" => "Интервал обновления",
            "How often the display receives new sensor values" => {
                "Как часто дисплей получает новые значения датчиков"
            }
            "Apply" => "Применить",
            "Diagnostics" => "Диагностика",
            "Interface" => "Интерфейс",
            "Last update" => "Последнее обновление",
            "Language" => "Язык",
            "Theme" => "Тема",
            "System" => "Системная",
            "Light" => "Светлая",
            "Dark" => "Тёмная",
            "The D-Bus worker has stopped" => "Служба D-Bus остановлена",
            "This daemon exposes a newer API; update ak620-control." => {
                "Служба использует более новую версию API; обновите ak620-control."
            }
            "Open settings" => "Открыть настройки",
            "Quit" => "Выйти",
            "Unavailable" => "Недоступно",
            _ => english,
        }
    }
    pub(crate) fn localize_error(self, error: &str) -> String {
        if self == Self::English {
            return error.to_owned();
        }
        error
            .replace(
                "device connection failed",
                "не удалось подключиться к устройству",
            )
            .replace("HID operation failed", "ошибка операции HID")
            .replace(
                "failed to open device with path",
                "не удалось открыть устройство по пути",
            )
            .replace("Permission denied", "доступ запрещён")
            .replace("os error", "ошибка ОС")
            .replace("display update failed", "не удалось обновить дисплей")
            .replace("metric update failed", "не удалось обновить метрики")
            .replace("metric discovery failed", "не удалось найти датчики")
            .replace(
                "configuration update failed",
                "не удалось обновить конфигурацию",
            )
            .replace(
                "Could not connect to the system bus",
                "не удалось подключиться к системной шине",
            )
            .replace(
                "Could not create the daemon proxy",
                "не удалось создать прокси службы",
            )
            .replace("ak620d is unavailable", "ak620d недоступна")
    }
    pub(crate) const fn temperature_choice(self, choice: TemperatureChoice) -> &'static str {
        match (self, choice) {
            (Self::English, TemperatureChoice::Celsius) => "Celsius",
            (Self::English, TemperatureChoice::Fahrenheit) => "Fahrenheit",
            (Self::Russian, TemperatureChoice::Celsius) => "Цельсий",
            (Self::Russian, TemperatureChoice::Fahrenheit) => "Фаренгейт",
        }
    }
    pub(crate) fn interval(self, milliseconds: u64) -> String {
        if milliseconds < 1_000 || milliseconds % 1_000 != 0 {
            return format!(
                "{milliseconds} {}",
                if self == Self::Russian { "мс" } else { "ms" }
            );
        }

        let seconds = milliseconds / 1_000;
        if self == Self::English {
            return format!(
                "{seconds} {}",
                if seconds == 1 { "second" } else { "seconds" }
            );
        }

        let word = match (seconds % 10, seconds % 100) {
            (1, 11) => "секунд",
            (1, _) => "секунда",
            (2..=4, 12..=14) => "секунд",
            (2..=4, _) => "секунды",
            _ => "секунд",
        };
        format!("{seconds} {word}")
    }
    pub(crate) fn tray_title(self, snapshot: &DaemonSnapshot) -> String {
        if snapshot.has_metrics {
            format!(
                "AK620 · {:.0}{} · {} W",
                snapshot.temperature_degrees,
                snapshot.temperature_unit.symbol(),
                snapshot.power_watts
            )
        } else {
            format!("AK620 · {}", self.text("Unavailable").to_lowercase())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeChoice {
    System,
    Light,
    Dark,
}
impl ThemeChoice {
    pub(crate) fn label(self, language: Language) -> &'static str {
        language.text(match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        })
    }
}
impl From<ThemeChoice> for egui::ThemePreference {
    fn from(value: ThemeChoice) -> Self {
        match value {
            ThemeChoice::System => Self::System,
            ThemeChoice::Light => Self::Light,
            ThemeChoice::Dark => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Preferences {
    pub(crate) language: Language,
    pub(crate) theme: ThemeChoice,
}
impl Preferences {
    pub(crate) fn load() -> Self {
        let defaults = Self {
            language: if env::var("LANG").is_ok_and(|value| value.starts_with("ru")) {
                Language::Russian
            } else {
                Language::English
            },
            theme: ThemeChoice::System,
        };
        let Ok(contents) = fs::read_to_string(path()) else {
            return defaults;
        };
        Self {
            language: if contents.contains("language=ru") {
                Language::Russian
            } else {
                Language::English
            },
            theme: if contents.contains("theme=light") {
                ThemeChoice::Light
            } else if contents.contains("theme=dark") {
                ThemeChoice::Dark
            } else {
                ThemeChoice::System
            },
        }
    }
    pub(crate) fn save(self) {
        let target = path();
        let Some(parent) = target.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_ok() {
            let language = if self.language == Language::Russian {
                "ru"
            } else {
                "en"
            };
            let theme = match self.theme {
                ThemeChoice::System => "system",
                ThemeChoice::Light => "light",
                ThemeChoice::Dark => "dark",
            };
            let _ = fs::write(target, format!("language={language}\ntheme={theme}\n"));
        }
    }
}
fn path() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ak620-linux/control-ui.conf")
}

#[cfg(test)]
mod tests {
    use super::Language;
    use crate::model::TemperatureChoice;

    #[test]
    fn russian_setting_values_are_localized() {
        assert_eq!(
            Language::Russian.temperature_choice(TemperatureChoice::Celsius),
            "Цельсий"
        );
        assert_eq!(
            Language::Russian.temperature_choice(TemperatureChoice::Fahrenheit),
            "Фаренгейт"
        );
        assert_eq!(Language::Russian.interval(250), "250 мс");
        assert_eq!(Language::Russian.interval(1_000), "1 секунда");
        assert_eq!(Language::Russian.interval(2_000), "2 секунды");
        assert_eq!(Language::Russian.interval(5_000), "5 секунд");
    }

    #[test]
    fn english_setting_values_are_localized() {
        assert_eq!(Language::English.interval(500), "500 ms");
        assert_eq!(Language::English.interval(1_000), "1 second");
        assert_eq!(Language::English.interval(10_000), "10 seconds");
    }
}
