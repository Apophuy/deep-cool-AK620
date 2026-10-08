//! AK620 DIGITAL PRO desktop settings and tray entry point.

mod client;
mod model;
mod preferences;
mod tray;
mod tray_icons;

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
use model::{DaemonSnapshot, SettingsDraft, TemperatureChoice};
use preferences::{Language, Preferences, ThemeChoice};
use time::{OffsetDateTime, UtcOffset};
use tray::WindowAction;

const APP_ID: &str = "io.github.ak620linux.Control";
const INSTALLED_EXECUTABLE: &str = "/usr/bin/ak620-control";
const INITIAL_WINDOW_SIZE: [f32; 2] = [1_050.0, 720.0];
const WINDOW_MARGIN_X: i8 = 20;
const WINDOW_MARGIN_Y: i8 = 16;
const UPDATE_INTERVALS_MS: [u64; 6] = [250, 500, 1_000, 2_000, 5_000, 10_000];

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
            .with_inner_size(INITIAL_WINDOW_SIZE)
            .with_min_inner_size([760.0, 520.0]),
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
    page: Page,
    cooler_texture: Option<egui::TextureHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Monitoring,
    System,
    Device,
    Settings,
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
            page: Page::Monitoring,
            cooler_texture: load_cooler_texture(context),
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

    fn show_navigation(&mut self, context: &egui::Context, language: Language) {
        egui::SidePanel::left("navigation")
            .resizable(false)
            .exact_width(72.0)
            .frame(
                egui::Frame::default()
                    .fill(context.style().visuals.extreme_bg_color)
                    .inner_margin(egui::Margin::symmetric(10, 16)),
            )
            .show(context, |ui| {
                navigation_logo(ui);
                ui.add_space(18.0);
                navigation_button(
                    ui,
                    &mut self.page,
                    Page::Monitoring,
                    language.text("Monitoring"),
                );
                navigation_button(
                    ui,
                    &mut self.page,
                    Page::System,
                    language.text("System information"),
                );
                navigation_button(ui, &mut self.page, Page::Device, language.text("Device"));
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    navigation_button(
                        ui,
                        &mut self.page,
                        Page::Settings,
                        language.text("Settings"),
                    );
                });
            });
    }

    fn show_monitoring(&self, ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
        ui.horizontal(|ui| {
            page_heading(ui, language.text("Monitoring"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                connection_badge(ui, snapshot, language);
            });
        });
        ui.add_space(12.0);

        if snapshot.has_gpu_metrics && ui.available_width() >= 720.0 {
            ui.columns(2, |columns| {
                cpu_overview_card(&mut columns[0], snapshot, language);
                gpu_overview_card(&mut columns[1], snapshot, language);
            });
        } else {
            cpu_overview_card(ui, snapshot, language);
            if snapshot.has_gpu_metrics {
                ui.add_space(10.0);
                gpu_overview_card(ui, snapshot, language);
            }
        }

        let show_memory = snapshot.memory_total_bytes > 0;
        let show_storage = !snapshot.storage_labels.is_empty();
        let show_network = snapshot.has_telemetry;
        if show_memory || show_storage || show_network {
            ui.add_space(10.0);
            if ui.available_width() >= 720.0 {
                ui.columns(3, |columns| {
                    if show_memory {
                        memory_overview_card(&mut columns[0], snapshot, language);
                    }
                    if show_storage {
                        storage_overview_card(&mut columns[1], snapshot, language);
                    }
                    if show_network {
                        network_overview_card(&mut columns[2], snapshot, language);
                    }
                });
            } else {
                if show_memory {
                    memory_overview_card(ui, snapshot, language);
                    ui.add_space(10.0);
                }
                if show_storage {
                    storage_overview_card(ui, snapshot, language);
                    ui.add_space(10.0);
                }
                if show_network {
                    network_overview_card(ui, snapshot, language);
                }
            }
        }
    }

    fn show_system(&self, ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
        page_heading(ui, language.text("Computer configuration"));
        ui.add_space(12.0);
        section_card(ui, language.text("System information"), |ui| {
            info_row(ui, language.text("Device name"), &snapshot.host_name);
            info_row(
                ui,
                language.text("Operating system"),
                &snapshot.operating_system,
            );
            info_row(ui, "CPU", &snapshot.cpu_name);
            info_row(ui, "GPU", &snapshot.gpu_name);
            info_row(ui, language.text("Motherboard"), &snapshot.motherboard_name);
            info_row(ui, language.text("Memory"), &snapshot.memory_description);
            if !snapshot.drive_models.is_empty() {
                ui.label(egui::RichText::new(language.text("Drives")).strong());
                for drive in &snapshot.drive_models {
                    ui.weak(drive);
                }
            }
        });
    }

    fn show_device(&self, ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
        page_heading(ui, language.text("Device"));
        ui.add_space(12.0);
        section_card(ui, "AK620 DIGITAL PRO", |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.heading(egui::RichText::new("AK620 DIGITAL PRO").size(28.0));
                    ui.weak(language.text("Digital air cooler"));
                    if snapshot.connected() && !snapshot.device_path.is_empty() {
                        ui.add_space(8.0);
                        ui.monospace(&snapshot.device_path);
                    }
                });
                if let Some(texture) = &self.cooler_texture {
                    let available = ui.available_width().min(330.0);
                    ui.add(
                        egui::Image::new(texture)
                            .max_width(available)
                            .max_height(190.0),
                    );
                }
            });
            if snapshot.has_metrics {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    device_metric(
                        ui,
                        language.text("CPU temperature"),
                        format!(
                            "{:.0} {}",
                            snapshot.temperature_degrees,
                            snapshot.temperature_unit.symbol()
                        ),
                    );
                    device_metric(
                        ui,
                        language.text("CPU frequency"),
                        format!("{:.1} GHz", f64::from(snapshot.frequency_mhz) / 1_000.0),
                    );
                    if snapshot.has_fan_rpm {
                        device_metric(
                            ui,
                            language.text("Fan speed"),
                            format!("{} RPM", snapshot.fan_rpm),
                        );
                    }
                });
            }
        });
    }

    fn show_settings(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
        snapshot: &DaemonSnapshot,
        language: Language,
    ) {
        page_heading(ui, language.text("Settings"));
        ui.add_space(12.0);
        section_card(ui, language.text("Display settings"), |ui| {
            let available_width = ui.available_width();
            let label_width = settings_label_width(ui, language);
            let control_width = available_width - label_width - 18.0;
            let stacked = control_width < 200.0;
            let previous_unit = self.settings.temperature_unit;

            if stacked {
                ui.label(language.text("Temperature unit"));
                temperature_selector(
                    ui,
                    &mut self.settings.temperature_unit,
                    language,
                    available_width,
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(language.text("Refresh interval"));
                    info_icon(ui).on_hover_text(
                        language.text("How often the display receives new sensor values"),
                    );
                });
                if interval_controls(
                    ui,
                    &mut self.settings.interval_ms,
                    language,
                    available_width,
                ) {
                    self.send(ClientCommand::SetUpdateIntervalMs(
                        self.settings.interval_ms,
                    ));
                }
            } else {
                egui::Grid::new("display-settings")
                    .num_columns(2)
                    .spacing([18.0, 12.0])
                    .show(ui, |ui| {
                        ui.label(language.text("Temperature unit"));
                        temperature_selector(
                            ui,
                            &mut self.settings.temperature_unit,
                            language,
                            control_width,
                        );
                        ui.end_row();
                        ui.horizontal(|ui| {
                            ui.label(language.text("Refresh interval"));
                            info_icon(ui).on_hover_text(
                                language.text("How often the display receives new sensor values"),
                            );
                        });
                        if interval_controls(
                            ui,
                            &mut self.settings.interval_ms,
                            language,
                            control_width,
                        ) {
                            self.send(ClientCommand::SetUpdateIntervalMs(
                                self.settings.interval_ms,
                            ));
                        }
                        ui.end_row();
                    });
            }
            if self.settings.temperature_unit != previous_unit {
                self.send(ClientCommand::SetTemperatureUnit(
                    self.settings.temperature_unit,
                ));
            }
        });

        ui.add_space(10.0);
        section_card(ui, language.text("Interface"), |ui| {
            let old_language = self.preferences.language;
            let old_theme = self.preferences.theme;
            ui.columns(2, |columns| {
                columns[0].label(language.text("Language"));
                let language_width = columns[0].available_width();
                styled_combo_box(
                    "language",
                    self.preferences.language.label(),
                    language_width,
                )
                .show_ui(&mut columns[0], |ui| {
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
                columns[1].label(language.text("Theme"));
                let theme_width = columns[1].available_width();
                styled_combo_box("theme", self.preferences.theme.label(language), theme_width)
                    .show_ui(&mut columns[1], |ui| {
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
            });
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                diagnostic_value(ui, "D-Bus API", snapshot.api_version.to_string());
                ui.separator();
                diagnostic_value(
                    ui,
                    language.text("Last update"),
                    format_update_time(snapshot.last_update_unix_seconds),
                );
            });
            if !snapshot.last_error.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 102, 82),
                    language.localize_error(&snapshot.last_error),
                );
            }
            if let Some(error) = &self.local_error {
                ui.colored_label(egui::Color32::RED, error);
            }
            if snapshot.api_version > 2 {
                ui.label(language.text("This daemon exposes a newer API; update ak620-control."));
            }
            if old_theme != self.preferences.theme {
                apply_theme(context, self.preferences.theme);
            }
            if old_language != self.preferences.language || old_theme != self.preferences.theme {
                self.preferences.save();
            }
        });
    }
}

impl eframe::App for ControlApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        let snapshot = self.shared.get();
        self.settings.sync(&snapshot);
        let language = self.preferences.language;
        self.show_navigation(context, language);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(context.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(WINDOW_MARGIN_X, WINDOW_MARGIN_Y)),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Monitoring => self.show_monitoring(ui, &snapshot, language),
                        Page::System => self.show_system(ui, &snapshot, language),
                        Page::Device => self.show_device(ui, &snapshot, language),
                        Page::Settings => {
                            self.show_settings(ui, context, &snapshot, language);
                        }
                    });
            });
        context.request_repaint_after(Duration::from_millis(500));
    }
}

fn load_cooler_texture(context: &egui::Context) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory(include_bytes!("../assets/ak620-cooler-green.png"))
        .ok()?
        .to_rgba8();
    let width = usize::try_from(image.width()).ok()?;
    let height = usize::try_from(image.height()).ok()?;
    let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], image.as_raw());
    Some(context.load_texture("ak620-cooler", color_image, egui::TextureOptions::LINEAR))
}

fn navigation_logo(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        let (response, painter) = ui.allocate_painter(egui::vec2(42.0, 30.0), egui::Sense::hover());
        let center = response.rect.center();
        painter.text(
            egui::pos2(center.x - 3.0, center.y),
            egui::Align2::CENTER_CENTER,
            "DC",
            egui::FontId::proportional(17.0),
            ui.visuals().text_color(),
        );
        let accent = rgb(35, 166, 157);
        painter.line_segment(
            [
                egui::pos2(center.x + 12.0, center.y - 7.0),
                egui::pos2(center.x + 12.0, center.y + 7.0),
            ],
            egui::Stroke::new(2.5_f32, accent),
        );
        painter.line_segment(
            [
                egui::pos2(center.x + 7.0, center.y),
                egui::pos2(center.x + 17.0, center.y),
            ],
            egui::Stroke::new(2.5_f32, accent),
        );
    });
}

fn navigation_button(ui: &mut egui::Ui, page: &mut Page, value: Page, label: &str) {
    let selected = *page == value;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.0), egui::Sense::click());
    let accent = rgb(35, 166, 157);
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            accent.gamma_multiply(0.22),
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.left() + 3.0, rect.bottom()),
            ),
            egui::CornerRadius::same(2),
            accent,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            ui.visuals().widgets.hovered.bg_fill,
        );
    }
    let color = if selected {
        accent
    } else {
        ui.visuals().weak_text_color()
    };
    paint_navigation_icon(ui.painter(), rect.center(), value, color);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(2.0),
            egui::CornerRadius::same(7),
            egui::Stroke::new(1.0_f32, accent),
            egui::StrokeKind::Inside,
        );
    }
    let clicked = response.clicked();
    response.on_hover_text(label);
    if clicked {
        *page = value;
    }
}

fn paint_navigation_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    page: Page,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.8_f32, color);
    match page {
        Page::Monitoring => {
            painter.add(egui::Shape::line(
                arc_points(center, 10.0, 155.0, 230.0, 24),
                stroke,
            ));
            painter.line_segment([center, egui::pos2(center.x + 5.5, center.y - 5.0)], stroke);
            painter.circle_filled(center, 1.8, color);
        }
        Page::System => {
            let screen = egui::Rect::from_center_size(
                egui::pos2(center.x, center.y - 2.0),
                egui::vec2(20.0, 14.0),
            );
            painter.rect_stroke(
                screen,
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x, screen.bottom()),
                    egui::pos2(center.x, center.y + 9.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y + 9.0),
                    egui::pos2(center.x + 5.0, center.y + 9.0),
                ],
                stroke,
            );
        }
        Page::Device => {
            for offset in [-6.0, 6.0] {
                let rect = egui::Rect::from_center_size(
                    egui::pos2(center.x, center.y + offset),
                    egui::vec2(20.0, 8.0),
                );
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(2),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.circle_filled(egui::pos2(rect.right() - 3.5, rect.center().y), 1.1, color);
            }
        }
        Page::Settings => {
            painter.circle_stroke(center, 5.0, stroke);
            painter.circle_filled(center, 1.7, color);
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::TAU / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + direction * 7.0, center + direction * 10.0],
                    stroke,
                );
            }
        }
    }
}

fn cpu_overview_card(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    hardware_overview_card(
        ui,
        "CPU",
        &snapshot.cpu_name,
        snapshot.has_metrics.then_some(snapshot.utilization_percent),
        snapshot
            .has_metrics
            .then_some(cpu_temperature_celsius(snapshot)),
        [
            (
                metric_with_unit(snapshot.has_metrics, snapshot.frequency_mhz, "MHz"),
                language.text("CPU frequency"),
            ),
            (
                if snapshot.has_metrics {
                    format!(
                        "{:.0} {}",
                        snapshot.temperature_degrees,
                        snapshot.temperature_unit.symbol()
                    )
                } else {
                    "—".to_owned()
                },
                language.text("CPU temperature"),
            ),
            (
                metric_with_unit(snapshot.has_metrics, snapshot.power_watts, "W"),
                language.text("CPU package power"),
            ),
        ],
        language,
    );
}

fn gpu_overview_card(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    hardware_overview_card(
        ui,
        "GPU",
        &snapshot.gpu_name,
        Some(snapshot.gpu_utilization_percent),
        Some(snapshot.gpu_temperature_celsius as f32),
        [
            (
                format!("{} MHz", snapshot.gpu_frequency_mhz),
                language.text("GPU frequency"),
            ),
            (
                format!("{:.0} °C", snapshot.gpu_temperature_celsius),
                language.text("GPU temperature"),
            ),
            (
                format!(
                    "{} / {}",
                    format_compact_bytes(snapshot.gpu_memory_used_bytes),
                    format_compact_bytes(snapshot.gpu_memory_total_bytes)
                ),
                language.text("Video memory"),
            ),
        ],
        language,
    );
}

fn hardware_overview_card(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    load_percent: Option<u8>,
    temperature_celsius: Option<f32>,
    details: [(String, &str); 3],
    language: Language,
) {
    dashboard_card(ui, 250.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(19.0).strong());
            if !subtitle.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(subtitle)
                                .size(13.0)
                                .color(ui.visuals().weak_text_color()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(subtitle);
                });
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            dual_gauge(ui, load_percent, temperature_celsius, language.text("Load"));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                for (value, label) in details {
                    overview_metric(ui, value, label);
                    ui.add_space(11.0);
                }
            });
        });
    });
}

fn dual_gauge(
    ui: &mut egui::Ui,
    load_percent: Option<u8>,
    temperature_celsius: Option<f32>,
    load_label: &str,
) {
    let (response, painter) = ui.allocate_painter(egui::vec2(190.0, 180.0), egui::Sense::hover());
    let center = egui::pos2(response.rect.center().x, response.rect.top() + 88.0);
    let start_degrees = 150.0;
    let sweep_degrees = 240.0;
    let track = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let accent = rgb(35, 166, 157);
    let temperature_color = rgb(82, 194, 187);
    painter.add(egui::Shape::line(
        arc_points(center, 74.0, start_degrees, sweep_degrees, 48),
        egui::Stroke::new(7.0_f32, track),
    ));
    painter.add(egui::Shape::line(
        arc_points(center, 61.0, start_degrees, sweep_degrees, 48),
        egui::Stroke::new(4.0_f32, track),
    ));
    if let Some(load) = load_percent {
        painter.add(egui::Shape::line(
            arc_points(
                center,
                74.0,
                start_degrees,
                sweep_degrees * gauge_fraction(f32::from(load), 100.0),
                48,
            ),
            egui::Stroke::new(7.0_f32, accent),
        ));
    }
    if let Some(temperature) = temperature_celsius {
        painter.add(egui::Shape::line(
            arc_points(
                center,
                61.0,
                start_degrees,
                sweep_degrees * gauge_fraction(temperature, 120.0),
                48,
            ),
            egui::Stroke::new(4.0_f32, temperature_color),
        ));
    }
    painter.text(
        egui::pos2(center.x, center.y - 10.0),
        egui::Align2::CENTER_CENTER,
        load_label,
        egui::FontId::proportional(13.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        egui::pos2(center.x - 3.0, center.y + 22.0),
        egui::Align2::CENTER_CENTER,
        load_percent.map_or_else(|| "—".to_owned(), |value| value.to_string()),
        egui::FontId::proportional(42.0),
        ui.visuals().text_color(),
    );
    if load_percent.is_some() {
        painter.text(
            egui::pos2(center.x + 31.0, center.y + 30.0),
            egui::Align2::CENTER_CENTER,
            "%",
            egui::FontId::proportional(14.0),
            ui.visuals().text_color(),
        );
    }
    painter.text(
        egui::pos2(response.rect.left() + 13.0, response.rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        "0 °C",
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        egui::pos2(response.rect.right() - 9.0, response.rect.bottom() - 4.0),
        egui::Align2::RIGHT_BOTTOM,
        "120 °C",
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

fn arc_points(
    center: egui::Pos2,
    radius: f32,
    start_degrees: f32,
    sweep_degrees: f32,
    segments: usize,
) -> Vec<egui::Pos2> {
    let segment_count = segments.max(1);
    (0..=segment_count)
        .map(|index| {
            let fraction = index as f32 / segment_count as f32;
            let angle = (start_degrees + sweep_degrees * fraction).to_radians();
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

fn gauge_fraction(value: f32, maximum: f32) -> f32 {
    if !value.is_finite() || maximum <= 0.0 {
        return 0.0;
    }
    (value / maximum).clamp(0.0, 1.0)
}

fn cpu_temperature_celsius(snapshot: &DaemonSnapshot) -> f32 {
    match snapshot.temperature_unit {
        TemperatureChoice::Celsius => snapshot.temperature_degrees as f32,
        TemperatureChoice::Fahrenheit => ((snapshot.temperature_degrees - 32.0) / 1.8) as f32,
    }
}

fn overview_metric(ui: &mut egui::Ui, value: String, label: &str) {
    ui.label(
        egui::RichText::new(value)
            .size(18.0)
            .strong()
            .color(rgb(35, 166, 157)),
    );
    ui.label(
        egui::RichText::new(label)
            .size(12.0)
            .color(ui.visuals().weak_text_color()),
    );
}

fn memory_overview_card(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    dashboard_card(ui, 225.0, |ui| {
        dashboard_card_title(ui, language.text("Memory"));
        ui.add_space(24.0);
        let percentage = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
        ui.label(
            egui::RichText::new(format!("{percentage}%"))
                .size(34.0)
                .strong(),
        );
        ui.weak(language.text("Usage"));
        ui.add_space(12.0);
        usage_bar(
            ui,
            snapshot.memory_used_bytes,
            snapshot.memory_total_bytes,
            format!(
                "{} / {}",
                format_compact_bytes(snapshot.memory_used_bytes),
                format_compact_bytes(snapshot.memory_total_bytes)
            ),
        );
    });
}

fn storage_overview_card(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    dashboard_card(ui, 225.0, |ui| {
        dashboard_card_title(ui, language.text("Storage"));
        ui.horizontal_wrapped(|ui| {
            io_rate(
                ui,
                language.text("Read"),
                snapshot.storage_read_bytes_per_second,
            );
            ui.separator();
            io_rate(
                ui,
                language.text("Write"),
                snapshot.storage_write_bytes_per_second,
            );
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .id_salt("storage-volumes")
            .max_height(138.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, label) in snapshot.storage_labels.iter().enumerate() {
                    let Some(used) = snapshot.storage_used_bytes.get(index) else {
                        continue;
                    };
                    let Some(total) = snapshot.storage_total_bytes.get(index) else {
                        continue;
                    };
                    ui.strong(label);
                    usage_bar(
                        ui,
                        *used,
                        *total,
                        format!(
                            "{} / {}",
                            format_compact_bytes(*used),
                            format_compact_bytes(*total)
                        ),
                    );
                    ui.add_space(5.0);
                }
            });
    });
}

fn network_overview_card(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    dashboard_card(ui, 225.0, |ui| {
        dashboard_card_title(ui, language.text("Network"));
        ui.add_space(24.0);
        network_rate(
            ui,
            false,
            language.text("Download"),
            snapshot.network_receive_bytes_per_second,
        );
        ui.add_space(18.0);
        network_rate(
            ui,
            true,
            language.text("Upload"),
            snapshot.network_transmit_bytes_per_second,
        );
    });
}

fn network_rate(ui: &mut egui::Ui, upward: bool, label: &str, bytes_per_second: u64) {
    ui.horizontal(|ui| {
        let (response, painter) = ui.allocate_painter(egui::vec2(28.0, 32.0), egui::Sense::hover());
        let accent = rgb(35, 166, 157);
        let center = response.rect.center();
        let direction = if upward { -1.0 } else { 1.0 };
        let tip = egui::pos2(center.x, center.y + direction * 8.0);
        let tail = egui::pos2(center.x, center.y - direction * 8.0);
        painter.line_segment([tail, tip], egui::Stroke::new(2.0_f32, accent));
        painter.line_segment(
            [tip, egui::pos2(tip.x - 4.5, tip.y - direction * 4.5)],
            egui::Stroke::new(2.0_f32, accent),
        );
        painter.line_segment(
            [tip, egui::pos2(tip.x + 4.5, tip.y - direction * 4.5)],
            egui::Stroke::new(2.0_f32, accent),
        );
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!("{}/s", format_bytes(bytes_per_second)))
                    .size(18.0)
                    .strong(),
            );
            ui.weak(label);
        });
    });
}

fn dashboard_card(
    ui: &mut egui::Ui,
    minimum_height: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(minimum_height);
            add_contents(ui);
        });
}

fn dashboard_card_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(18.0).strong());
    ui.add_space(6.0);
}

fn percent(used: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let rounded = (u128::from(used) * 100 + u128::from(total) / 2) / u128::from(total);
    u64::try_from(rounded.min(100)).unwrap_or(100)
}

fn format_compact_bytes(bytes: u64) -> String {
    format_bytes(bytes).replace(".0 ", " ")
}

fn page_heading(ui: &mut egui::Ui, title: &str) {
    ui.heading(egui::RichText::new(title).size(28.0));
}

fn connection_badge(ui: &mut egui::Ui, snapshot: &DaemonSnapshot, language: Language) {
    ui.horizontal_wrapped(|ui| {
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
        let (response, painter) = ui.allocate_painter(egui::vec2(14.0, 14.0), egui::Sense::hover());
        painter.circle_filled(response.rect.center(), 5.5, color);
        ui.label(egui::RichText::new(text).strong().color(color));
        if !snapshot.device_path.is_empty() {
            ui.weak(&snapshot.device_path);
        }
    });
}

fn usage_bar(ui: &mut egui::Ui, used: u64, total: u64, text: String) {
    if total == 0 {
        return;
    }
    let fraction = (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
    ui.add(
        egui::ProgressBar::new(fraction)
            .show_percentage()
            .fill(rgb(35, 166, 157))
            .desired_height(28.0)
            .text(text),
    );
}

fn io_rate(ui: &mut egui::Ui, label: &str, bytes_per_second: u64) {
    ui.weak(label);
    ui.strong(format!("{}/s", format_bytes(bytes_per_second)));
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(label).strong());
    ui.weak(value);
    ui.add_space(8.0);
}

fn device_metric(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(value).strong().size(20.0));
        ui.weak(label);
    });
    ui.add_space(22.0);
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn apply_theme(context: &egui::Context, choice: ThemeChoice) {
    context.set_visuals_of(egui::Theme::Light, app_visuals(false));
    context.set_visuals_of(egui::Theme::Dark, app_visuals(true));
    context.all_styles_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 36.0;
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.icon_width = 14.0;
    });
    context.set_theme(choice);
}

fn app_visuals(dark: bool) -> egui::Visuals {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let (panel, card, control, border, text, muted, hover, active, accent) = if dark {
        (
            rgb(17, 22, 30),
            rgb(25, 33, 45),
            rgb(31, 41, 55),
            rgb(54, 68, 86),
            rgb(238, 243, 249),
            rgb(157, 169, 184),
            rgb(42, 55, 72),
            rgb(50, 65, 84),
            rgb(86, 145, 255),
        )
    } else {
        (
            rgb(243, 246, 250),
            rgb(255, 255, 255),
            rgb(247, 249, 252),
            rgb(211, 219, 230),
            rgb(31, 42, 55),
            rgb(103, 116, 134),
            rgb(238, 243, 249),
            rgb(226, 234, 244),
            rgb(48, 111, 229),
        )
    };

    visuals.panel_fill = panel;
    visuals.window_fill = panel;
    visuals.faint_bg_color = card;
    visuals.extreme_bg_color = control;
    visuals.override_text_color = Some(text);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, border);
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);
    visuals.widgets.noninteractive.bg_fill = card;
    visuals.widgets.noninteractive.weak_bg_fill = card;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, muted);
    for (widget, fill) in [
        (&mut visuals.widgets.inactive, control),
        (&mut visuals.widgets.hovered, hover),
        (&mut visuals.widgets.active, active),
        (&mut visuals.widgets.open, active),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = egui::Stroke::new(1.0_f32, border);
        widget.fg_stroke = egui::Stroke::new(1.5_f32, text);
        widget.corner_radius = egui::CornerRadius::same(7);
        widget.expansion = 0.0;
    }
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals
}

const fn rgb(red: u8, green: u8, blue: u8) -> egui::Color32 {
    egui::Color32::from_rgb(red, green, blue)
}

fn settings_label_width(ui: &egui::Ui, language: Language) -> f32 {
    [
        language.text("Temperature unit"),
        language.text("Refresh interval"),
    ]
    .into_iter()
    .map(|label| text_width(ui, label, 14.0))
    .fold(0.0_f32, f32::max)
}

fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(size),
                ui.visuals().text_color(),
            )
            .size()
            .x
    })
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.inactive.bg_stroke.color,
        ))
        .outer_margin(egui::Margin {
            left: 0,
            right: 8,
            top: 0,
            bottom: 0,
        })
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(18.0).strong());
            ui.add_space(8.0);
            add_contents(ui);
        });
}

fn diagnostic_value(ui: &mut egui::Ui, label: &str, value: String) {
    ui.weak(label);
    ui.monospace(value);
}

fn format_update_time(timestamp: u64) -> String {
    if timestamp == 0 {
        return "—".to_owned();
    }
    let Ok(timestamp) = i64::try_from(timestamp) else {
        return "—".to_owned();
    };
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "—".to_owned();
    };
    let offset = UtcOffset::local_offset_at(datetime).unwrap_or(UtcOffset::UTC);
    format_time_with_offset(datetime, offset)
}

fn format_time_with_offset(datetime: OffsetDateTime, offset: UtcOffset) -> String {
    let local = datetime.to_offset(offset);
    format!(
        "{:02}:{:02}:{:02}",
        local.hour(),
        local.minute(),
        local.second()
    )
}

fn temperature_selector(
    ui: &mut egui::Ui,
    value: &mut TemperatureChoice,
    language: Language,
    width: f32,
) {
    styled_combo_box(
        "temperature-unit",
        language.temperature_choice(*value),
        width,
    )
    .show_ui(ui, |ui| {
        ui.selectable_value(
            value,
            TemperatureChoice::Celsius,
            language.temperature_choice(TemperatureChoice::Celsius),
        );
        ui.selectable_value(
            value,
            TemperatureChoice::Fahrenheit,
            language.temperature_choice(TemperatureChoice::Fahrenheit),
        );
    });
}

fn interval_controls(ui: &mut egui::Ui, value: &mut u64, language: Language, width: f32) -> bool {
    let button_width = (text_width(ui, language.text("Apply"), 15.0) + 32.0).clamp(96.0, 116.0);
    let combo_width = (width - button_width - ui.spacing().item_spacing.x).max(88.0);
    let mut apply = false;
    ui.allocate_ui_with_layout(
        egui::vec2(width, 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            styled_combo_box("update-interval", language.interval(*value), combo_width).show_ui(
                ui,
                |ui| {
                    for interval in UPDATE_INTERVALS_MS {
                        ui.selectable_value(value, interval, language.interval(interval));
                    }
                },
            );
            apply = primary_button(ui, language.text("Apply"), [button_width, 36.0]).clicked();
        },
    );
    apply
}

fn styled_combo_box(
    id_salt: impl std::hash::Hash,
    selected_text: impl Into<egui::WidgetText>,
    width: f32,
) -> egui::ComboBox {
    egui::ComboBox::from_id_salt(id_salt)
        .width(width)
        .selected_text(selected_text)
        .icon(combo_arrow)
}

fn combo_arrow(
    ui: &egui::Ui,
    rect: egui::Rect,
    visuals: &egui::style::WidgetVisuals,
    is_open: bool,
    _placement: egui::AboveOrBelow,
) {
    let center = rect.center();
    let half_width = 4.0;
    let half_height = 2.5;
    let points = if is_open {
        vec![
            egui::pos2(center.x - half_width, center.y + half_height),
            egui::pos2(center.x, center.y - half_height),
            egui::pos2(center.x + half_width, center.y + half_height),
        ]
    } else {
        vec![
            egui::pos2(center.x - half_width, center.y - half_height),
            egui::pos2(center.x, center.y + half_height),
            egui::pos2(center.x + half_width, center.y - half_height),
        ]
    };
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(1.8_f32, visuals.fg_stroke.color),
    ));
}

fn info_icon(ui: &mut egui::Ui) -> egui::Response {
    let (response, painter) = ui.allocate_painter(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let center = response.rect.center();
    let color = ui.visuals().weak_text_color();
    painter.circle_stroke(center, 6.0, egui::Stroke::new(1.2_f32, color));
    painter.circle_filled(egui::pos2(center.x, center.y - 2.5), 0.9, color);
    painter.line_segment(
        [
            egui::pos2(center.x, center.y),
            egui::pos2(center.x, center.y + 3.5),
        ],
        egui::Stroke::new(1.4_f32, color),
    );
    response
}

fn primary_button(ui: &mut egui::Ui, text: &str, size: [f32; 2]) -> egui::Response {
    let accent = ui.visuals().selection.bg_fill;
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.weak_bg_fill = accent;
        ui.visuals_mut().widgets.inactive.bg_fill = accent;
        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = accent.gamma_multiply(1.15);
        ui.visuals_mut().widgets.hovered.bg_fill = accent.gamma_multiply(1.15);
        ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.active.weak_bg_fill = accent.gamma_multiply(0.85);
        ui.visuals_mut().widgets.active.bg_fill = accent.gamma_multiply(0.85);
        ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
        ui.add_sized(
            size,
            egui::Button::new(
                egui::RichText::new(text)
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .corner_radius(egui::CornerRadius::same(7)),
        )
    })
    .inner
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
    use super::{
        INSTALLED_EXECUTABLE, format_time_with_offset, format_update_time, gauge_fraction,
        launch_executable, metric_with_unit, percent, should_launch_window,
    };
    use crate::tray::WindowAction;
    use std::path::Path;
    use time::{OffsetDateTime, UtcOffset};
    #[test]
    fn metric_placeholder_is_used_without_a_daemon_value() {
        assert_eq!(metric_with_unit(false, 42, "W"), "—");
        assert_eq!(metric_with_unit(true, 42, "W"), "42 W");
    }
    #[test]
    fn dashboard_indicators_clamp_and_round_values() {
        assert_eq!(gauge_fraction(-1.0, 100.0), 0.0);
        assert_eq!(gauge_fraction(50.0, 100.0), 0.5);
        assert_eq!(gauge_fraction(150.0, 100.0), 1.0);
        assert_eq!(gauge_fraction(f32::NAN, 100.0), 0.0);
        assert_eq!(percent(19, 64), 30);
        assert_eq!(percent(1, 0), 0);
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
    #[test]
    fn last_update_has_a_fixed_width_clock_format() {
        let offset = UtcOffset::from_hms(3, 0, 0).unwrap_or(UtcOffset::UTC);
        assert_eq!(
            format_time_with_offset(OffsetDateTime::UNIX_EPOCH, offset),
            "03:00:00"
        );
        assert_eq!(format_update_time(0), "—");
    }
}
