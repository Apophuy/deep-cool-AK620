//! AK620 DIGITAL PRO desktop settings and tray entry point.

mod client;
mod model;
mod preferences;
mod tray;

use std::{
    env,
    error::Error,
    path::PathBuf,
    process::{Child, Command, ExitCode},
    sync::mpsc,
    time::Duration,
};

use client::{ClientCommand, SharedSnapshot};
use eframe::egui;
use model::{SettingsDraft, TemperatureChoice};
use preferences::{Language, Preferences, ThemeChoice};
use tray::WindowAction;

const APP_ID: &str = "io.github.ak620linux.Control";
const INSTALLED_EXECUTABLE: &str = "/usr/bin/ak620-control";

fn main() -> ExitCode {
    let result = match env::args().nth(1).as_deref() {
        None => run_tray(true),
        Some("--window") => run_window().map_err(Into::into),
        Some("--tray") => run_tray(false),
        Some(argument) => Err(format!("unknown argument: {argument}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("AK620 Control: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_window() -> eframe::Result<()> {
    let shared = SharedSnapshot::default();
    let client_commands = client::spawn_worker(shared.clone());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([640.0, 640.0])
            .with_min_inner_size([480.0, 360.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "AK620 DIGITAL PRO",
        options,
        Box::new(move |creation| {
            Ok(Box::new(ControlApp::new(
                &creation.egui_ctx,
                shared,
                client_commands,
            )))
        }),
    )
}

fn run_tray(open_window_at_start: bool) -> Result<(), Box<dyn Error>> {
    let shared = SharedSnapshot::default();
    let _client_commands = client::spawn_worker(shared.clone());
    let (actions_sender, actions) = mpsc::channel();
    tray::spawn(shared, actions_sender)?;
    let mut window = None;
    if open_window_at_start {
        show_window(&mut window)?;
    }
    while let Ok(action) = actions.recv() {
        match action {
            WindowAction::Show => {
                if let Err(error) = show_window(&mut window) {
                    eprintln!("AK620 Control could not open settings: {error}");
                }
            }
            WindowAction::Quit => {
                if let Err(error) = stop_window(&mut window) {
                    eprintln!("AK620 Control could not stop settings: {error}");
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

fn show_window(window: &mut Option<Child>) -> Result<(), Box<dyn Error>> {
    let window_running = match window.as_mut() {
        Some(child) => child.try_wait()?.is_none(),
        None => false,
    };
    if should_launch_window(window_running, WindowAction::Show) {
        *window = Some(Command::new(launch_executable()).arg("--window").spawn()?);
    }
    Ok(())
}
fn should_launch_window(window_running: bool, action: WindowAction) -> bool {
    matches!(action, WindowAction::Show) && !window_running
}
fn stop_window(window: &mut Option<Child>) -> Result<(), Box<dyn Error>> {
    let Some(mut child) = window.take() else {
        return Ok(());
    };
    if child.try_wait()?.is_none() {
        child.kill()?;
        child.wait()?;
    }
    Ok(())
}
fn launch_executable() -> PathBuf {
    env::current_exe()
        .ok()
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(INSTALLED_EXECUTABLE))
}

struct ControlApp {
    shared: SharedSnapshot,
    client_commands: mpsc::Sender<ClientCommand>,
    settings: SettingsDraft,
    preferences: Preferences,
    local_error: Option<String>,
    last_content_height: f32,
}

impl ControlApp {
    fn new(
        context: &egui::Context,
        shared: SharedSnapshot,
        client_commands: mpsc::Sender<ClientCommand>,
    ) -> Self {
        let snapshot = shared.get();
        let preferences = Preferences::load();
        apply_theme(context, preferences.theme);
        Self {
            settings: SettingsDraft::new(&snapshot),
            shared,
            client_commands,
            preferences,
            local_error: None,
            last_content_height: 0.0,
        }
    }
    fn send(&mut self, command: ClientCommand) {
        if self.client_commands.send(command).is_err() {
            self.local_error = Some(
                self.preferences
                    .language
                    .text("The D-Bus worker has stopped")
                    .to_owned(),
            );
        }
    }
}

impl eframe::App for ControlApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        let snapshot = self.shared.get();
        self.settings.sync(&snapshot);
        let language = self.preferences.language;
        let mut content_height = 0.0;
        egui::CentralPanel::default()
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(16, 14)))
            .show(context, |ui| {
                ui.heading(egui::RichText::new("AK620 DIGITAL PRO").size(28.0));
                ui.horizontal(|ui| {
                    let (color, text) = if snapshot.connected() {
                        (
                            egui::Color32::from_rgb(45, 205, 110),
                            language.text("Connected"),
                        )
                    } else {
                        (
                            egui::Color32::from_rgb(220, 85, 70),
                            language.text("Disconnected"),
                        )
                    };
                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    painter.circle_filled(response.rect.center(), 5.5, color);
                    ui.label(egui::RichText::new(text).strong().color(color));
                    if !snapshot.device_path.is_empty() {
                        ui.weak(&snapshot.device_path);
                    }
                });
                ui.add_space(12.0);
                ui.columns(2, |columns| {
                    metric_value(
                        &mut columns[0],
                        language.text("Temperature"),
                        if snapshot.has_metrics {
                            format!(
                                "{:.0} {}",
                                snapshot.temperature_degrees,
                                snapshot.temperature_unit.symbol()
                            )
                        } else {
                            "—".to_owned()
                        },
                        egui::Color32::from_rgb(255, 135, 92),
                    );
                    metric_value(
                        &mut columns[1],
                        language.text("CPU utilization"),
                        metric_with_unit(snapshot.has_metrics, snapshot.utilization_percent, "%"),
                        egui::Color32::from_rgb(92, 181, 255),
                    );
                });
                ui.add_space(16.0);
                ui.columns(2, |columns| {
                    metric_value(
                        &mut columns[0],
                        language.text("CPU package power"),
                        metric_with_unit(snapshot.has_metrics, snapshot.power_watts, "W"),
                        egui::Color32::from_rgb(64, 211, 125),
                    );
                    metric_value(
                        &mut columns[1],
                        language.text("Highest core frequency"),
                        metric_with_unit(snapshot.has_metrics, snapshot.frequency_mhz, "MHz"),
                        egui::Color32::from_rgb(180, 132, 255),
                    );
                });
                ui.add_space(16.0);
                section_card(ui, language.text("Display settings"), |ui| {
                    ui.label(
                        egui::RichText::new(
                            language.text("Choose how values are shown on the cooler."),
                        )
                        .weak(),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(language.text("Temperature unit"));
                        let previous = self.settings.temperature_unit;
                        egui::ComboBox::from_id_salt("temperature-unit")
                            .width(190.0)
                            .selected_text(self.settings.temperature_unit.to_string())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.temperature_unit,
                                    TemperatureChoice::Celsius,
                                    "Celsius (°C)",
                                );
                                ui.selectable_value(
                                    &mut self.settings.temperature_unit,
                                    TemperatureChoice::Fahrenheit,
                                    "Fahrenheit (°F)",
                                );
                            });
                        if self.settings.temperature_unit != previous {
                            self.send(ClientCommand::SetTemperatureUnit(
                                self.settings.temperature_unit,
                            ));
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(language.text("Refresh interval"));
                            ui.weak(
                                language.text("How often the display receives new sensor values"),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_sized(
                                    [112.0, 34.0],
                                    egui::Button::new(
                                        egui::RichText::new(language.text("Apply")).strong(),
                                    ),
                                )
                                .clicked()
                            {
                                self.send(ClientCommand::SetUpdateIntervalMs(
                                    self.settings.interval_ms,
                                ));
                            }
                            ui.add_sized(
                                [120.0, 34.0],
                                egui::DragValue::new(&mut self.settings.interval_ms)
                                    .range(250..=10_000)
                                    .speed(50)
                                    .suffix(" ms"),
                            );
                        });
                    });
                });
                ui.add_space(16.0);
                section_card(ui, language.text("Diagnostics"), |ui| {
                    egui::Grid::new("diagnostics")
                        .num_columns(2)
                        .spacing([24.0, 10.0])
                        .show(ui, |ui| {
                            ui.weak("D-Bus API");
                            ui.monospace(snapshot.api_version.to_string());
                            ui.end_row();
                            ui.weak(language.text("Last update"));
                            ui.monospace(if snapshot.last_update_unix_seconds == 0 {
                                "—".to_owned()
                            } else {
                                snapshot.last_update_unix_seconds.to_string()
                            });
                            ui.end_row();
                        });
                    if !snapshot.last_error.is_empty() {
                        ui.add_space(8.0);
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 102, 82),
                            language.localize_error(&snapshot.last_error),
                        );
                    }
                    if let Some(error) = &self.local_error {
                        ui.colored_label(egui::Color32::RED, error);
                    }
                    if snapshot.api_version > 1 {
                        ui.label(
                            language.text("This daemon exposes a newer API; update ak620-control."),
                        );
                    }
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.label(language.text("Language"));
                    let old_language = self.preferences.language;
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(self.preferences.language.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.preferences.language,
                                Language::English,
                                "English",
                            );
                            ui.selectable_value(
                                &mut self.preferences.language,
                                Language::Russian,
                                "Русский",
                            );
                        });
                    ui.separator();
                    ui.label(language.text("Theme"));
                    let old_theme = self.preferences.theme;
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(self.preferences.theme.label(language))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.preferences.theme,
                                ThemeChoice::System,
                                language.text("System"),
                            );
                            ui.selectable_value(
                                &mut self.preferences.theme,
                                ThemeChoice::Light,
                                language.text("Light"),
                            );
                            ui.selectable_value(
                                &mut self.preferences.theme,
                                ThemeChoice::Dark,
                                language.text("Dark"),
                            );
                        });
                    if old_theme != self.preferences.theme {
                        apply_theme(context, self.preferences.theme);
                    }
                    if old_language != self.preferences.language
                        || old_theme != self.preferences.theme
                    {
                        self.preferences.save();
                    }
                });
                content_height = ui.min_rect().height();
            });
        let desired_height = (content_height + 28.0).ceil();
        if (desired_height - self.last_content_height).abs() > 1.0 {
            context.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                640.0,
                desired_height,
            )));
            self.last_content_height = desired_height;
        }
        context.request_repaint_after(Duration::from_millis(500));
    }
}

fn apply_theme(context: &egui::Context, choice: ThemeChoice) {
    context.set_theme(choice);
    let mut style = (*context.style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    context.set_style(style);
}
fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.inactive.bg_stroke.color,
        ))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).size(18.0).strong());
            ui.add_space(8.0);
            add_contents(ui);
        });
}
fn metric_value(ui: &mut egui::Ui, title: &str, value: String, color: egui::Color32) {
    ui.set_min_width(220.0);
    ui.label(
        egui::RichText::new(title)
            .color(egui::Color32::from_gray(160))
            .size(14.0),
    );
    ui.add_space(4.0);
    ui.label(egui::RichText::new(value).size(32.0).strong().color(color));
}
fn metric_with_unit(value_available: bool, value: impl std::fmt::Display, unit: &str) -> String {
    if value_available {
        format!("{value} {unit}")
    } else {
        "—".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{INSTALLED_EXECUTABLE, launch_executable, metric_with_unit, should_launch_window};
    use crate::tray::WindowAction;
    use std::path::Path;
    #[test]
    fn metric_placeholder_is_used_without_a_daemon_value() {
        assert_eq!(metric_with_unit(false, 42, "W"), "—");
        assert_eq!(metric_with_unit(true, 42, "W"), "42 W");
    }
    #[test]
    fn tray_launches_a_window_only_when_none_is_running() {
        assert!(should_launch_window(false, WindowAction::Show));
        assert!(!should_launch_window(true, WindowAction::Show));
    }
    #[test]
    fn launch_target_exists_for_the_running_binary_or_installed_path() {
        let executable = launch_executable();
        assert!(executable.exists() || executable == Path::new(INSTALLED_EXECUTABLE));
    }
}
