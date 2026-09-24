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
use time::{OffsetDateTime, UtcOffset};
use tray::WindowAction;

const APP_ID: &str = "io.github.ak620linux.Control";
const INSTALLED_EXECUTABLE: &str = "/usr/bin/ak620-control";
const INITIAL_WINDOW_SIZE: [f32; 2] = [500.0, 620.0];
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
            .with_min_inner_size([360.0, 420.0]),
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
            .frame(
                egui::Frame::default()
                    .fill(context.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(WINDOW_MARGIN_X, WINDOW_MARGIN_Y)),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let content_top = ui.cursor().top();
                        let metric_colors = metric_colors(ui.visuals().dark_mode);
                        ui.horizontal_wrapped(|ui| {
                            ui.heading(egui::RichText::new("AK620 DIGITAL PRO").size(26.0));
                            ui.add_space(8.0);
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
                        ui.add_space(10.0);
                        responsive_metrics(
                            ui,
                            [
                                (
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
                                    metric_colors[0],
                                ),
                                (
                                    language.text("CPU utilization"),
                                    metric_with_unit(
                                        snapshot.has_metrics,
                                        snapshot.utilization_percent,
                                        "%",
                                    ),
                                    metric_colors[1],
                                ),
                                (
                                    language.text("CPU package power"),
                                    metric_with_unit(
                                        snapshot.has_metrics,
                                        snapshot.power_watts,
                                        "W",
                                    ),
                                    metric_colors[2],
                                ),
                                (
                                    language.text("Highest core frequency"),
                                    metric_with_unit(
                                        snapshot.has_metrics,
                                        snapshot.frequency_mhz,
                                        "MHz",
                                    ),
                                    metric_colors[3],
                                ),
                            ],
                        );
                        ui.add_space(8.0);
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
                                        language.text(
                                            "How often the display receives new sensor values",
                                        ),
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
                                            info_icon(ui).on_hover_text(language.text(
                                                "How often the display receives new sensor values",
                                            ));
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
                        ui.add_space(8.0);
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
                                styled_combo_box(
                                    "theme",
                                    self.preferences.theme.label(language),
                                    theme_width,
                                )
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
                            ui.add_space(2.0);
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
                                ui.add_space(6.0);
                                ui.colored_label(
                                    egui::Color32::from_rgb(230, 102, 82),
                                    language.localize_error(&snapshot.last_error),
                                );
                            }
                            if let Some(error) = &self.local_error {
                                ui.colored_label(egui::Color32::RED, error);
                            }
                            if snapshot.api_version > 1 {
                                ui.label(language.text(
                                    "This daemon exposes a newer API; update ak620-control.",
                                ));
                            }
                            if old_theme != self.preferences.theme {
                                apply_theme(context, self.preferences.theme);
                            }
                            if old_language != self.preferences.language
                                || old_theme != self.preferences.theme
                            {
                                self.preferences.save();
                            }
                        });
                        ui.add_space(8.0);
                        content_height = ui.cursor().top() - content_top;
                    });
            });
        let desired_height = (content_height + f32::from(WINDOW_MARGIN_Y) * 2.0).ceil();
        if (desired_height - self.last_content_height).abs() > 1.0 {
            context.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                context.input(|input| {
                    input
                        .viewport()
                        .inner_rect
                        .map_or(INITIAL_WINDOW_SIZE[0], |rect| rect.width())
                }),
                desired_height,
            )));
            self.last_content_height = desired_height;
        }
        context.request_repaint_after(Duration::from_millis(500));
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
    visuals.window_stroke = egui::Stroke::new(1.0, border);
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    visuals.widgets.noninteractive.bg_fill = card;
    visuals.widgets.noninteractive.weak_bg_fill = card;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, muted);
    for (widget, fill) in [
        (&mut visuals.widgets.inactive, control),
        (&mut visuals.widgets.hovered, hover),
        (&mut visuals.widgets.active, active),
        (&mut visuals.widgets.open, active),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = egui::Stroke::new(1.0, border);
        widget.fg_stroke = egui::Stroke::new(1.5, text);
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

fn metric_colors(dark: bool) -> [egui::Color32; 4] {
    if dark {
        [
            rgb(255, 143, 99),
            rgb(101, 183, 255),
            rgb(75, 216, 136),
            rgb(188, 144, 255),
        ]
    } else {
        [
            rgb(201, 75, 27),
            rgb(31, 105, 180),
            rgb(20, 130, 73),
            rgb(116, 68, 176),
        ]
    }
}

fn responsive_metrics(ui: &mut egui::Ui, metrics: [(&str, String, egui::Color32); 4]) {
    let [first, second, third, fourth] = metrics;
    if ui.available_width() >= 360.0 {
        metric_row(ui, first, second);
        ui.add_space(8.0);
        metric_row(ui, third, fourth);
    } else {
        for (index, (title, value, color)) in [first, second, third, fourth].into_iter().enumerate()
        {
            metric_card(ui, title, value, color);
            if index != 3 {
                ui.add_space(8.0);
            }
        }
    }
}

fn metric_row(
    ui: &mut egui::Ui,
    left: (&str, String, egui::Color32),
    right: (&str, String, egui::Color32),
) {
    ui.columns(2, |columns| {
        metric_card(&mut columns[0], left.0, left.1, left.2);
        metric_card(&mut columns[1], right.0, right.1, right.2);
    });
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
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

fn metric_card(ui: &mut egui::Ui, title: &str, value: String, color: egui::Color32) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(50.0);
            ui.label(
                egui::RichText::new(title)
                    .color(ui.visuals().weak_text_color())
                    .size(13.0),
            );
            ui.add_space(3.0);
            ui.label(egui::RichText::new(value).size(28.0).strong().color(color));
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
        egui::Stroke::new(1.8, visuals.fg_stroke.color),
    ));
}

fn info_icon(ui: &mut egui::Ui) -> egui::Response {
    let (response, painter) = ui.allocate_painter(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let center = response.rect.center();
    let color = ui.visuals().weak_text_color();
    painter.circle_stroke(center, 6.0, egui::Stroke::new(1.2, color));
    painter.circle_filled(egui::pos2(center.x, center.y - 2.5), 0.9, color);
    painter.line_segment(
        [
            egui::pos2(center.x, center.y),
            egui::pos2(center.x, center.y + 3.5),
        ],
        egui::Stroke::new(1.4, color),
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
        INSTALLED_EXECUTABLE, format_time_with_offset, format_update_time, launch_executable,
        metric_with_unit, should_launch_window,
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
