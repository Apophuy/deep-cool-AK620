use std::{sync::mpsc, thread, time::Duration};

use ksni::{Category, Status, ToolTip, Tray, blocking::TrayMethods, menu::StandardItem};

use crate::{client::SharedSnapshot, model::DaemonSnapshot, preferences::Preferences};

#[derive(Debug, Clone, Copy)]
pub(crate) enum WindowAction {
    Show,
    Quit,
}

pub(crate) struct Ak620Tray {
    pub(crate) snapshot: DaemonSnapshot,
    actions: mpsc::Sender<WindowAction>,
    preferences: Preferences,
}

impl Ak620Tray {
    fn send(&self, action: WindowAction) {
        let _ = self.actions.send(action);
    }
}

impl Tray for Ak620Tray {
    fn id(&self) -> String {
        "ak620-control".to_owned()
    }

    fn category(&self) -> Category {
        Category::Hardware
    }

    fn title(&self) -> String {
        self.preferences.language.tray_title(&self.snapshot)
    }

    fn status(&self) -> Status {
        if self.snapshot.connected() {
            Status::Active
        } else {
            Status::NeedsAttention
        }
    }

    fn icon_name(&self) -> String {
        "io.github.ak620linux.Control".to_owned()
    }

    fn attention_icon_name(&self) -> String {
        "io.github.ak620linux.Control-attention".to_owned()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: self.icon_name(),
            title: "AK620 DIGITAL PRO".to_owned(),
            description: if self.snapshot.connected() {
                self.preferences.language.tray_title(&self.snapshot)
            } else {
                self.preferences
                    .language
                    .localize_error(&self.snapshot.last_error)
            },
            ..ToolTip::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(WindowAction::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let metrics = if self.snapshot.has_metrics {
            format!(
                "{:.0}{}  ·  {} W  ·  {}%  ·  {} MHz",
                self.snapshot.temperature_degrees,
                self.snapshot.temperature_unit.symbol(),
                self.snapshot.power_watts,
                self.snapshot.utilization_percent,
                self.snapshot.frequency_mhz
            )
        } else {
            self.preferences.language.text("Unavailable").to_owned()
        };

        vec![
            StandardItem {
                label: metrics,
                enabled: false,
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: self.preferences.language.text("Open settings").to_owned(),
                icon_name: "configure".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(WindowAction::Show)),
                ..StandardItem::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.preferences.language.text("Quit").to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(WindowAction::Quit)),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

pub(crate) fn spawn(
    shared: SharedSnapshot,
    actions: mpsc::Sender<WindowAction>,
) -> Result<(), ksni::Error> {
    let handle = Ak620Tray {
        snapshot: shared.get(),
        actions,
        preferences: Preferences::load(),
    }
    .spawn()?;
    thread::Builder::new()
        .name("ak620-tray-updater".to_owned())
        .spawn(move || {
            loop {
                let snapshot = shared.get();
                let preferences = Preferences::load();
                if handle
                    .update(move |tray| {
                        tray.snapshot = snapshot;
                        tray.preferences = preferences;
                    })
                    .is_none()
                {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("could not start tray updater thread");
    Ok(())
}
