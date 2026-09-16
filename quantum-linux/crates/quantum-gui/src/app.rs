use crate::device::{self, Command, Link, Pending, Shared, Snapshot};
use eframe::egui::{self, Color32, RichText};
use jbl_quantum::{Anc, Colour, Effect, Sidetone, Zone, ZoneLighting, MAX_SLOTS};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Sender};

const ACCENT: Color32 = Color32::from_rgb(0xff, 0x5a, 0x00);
const WARN: Color32 = Color32::from_rgb(0xe8, 0x4a, 0x3a);

/// The lighting the user last set up. The device cannot report its lighting
/// configuration, so the app remembers it instead.
#[derive(Clone, Serialize, Deserialize)]
struct ZoneEdit {
    brightness: u8,
    colours: Vec<[u8; 3]>,
    effect: u8,
}

impl Default for ZoneEdit {
    fn default() -> Self {
        Self { brightness: 100, colours: vec![[0xff, 0x5a, 0x00]], effect: Effect::FADE.0 }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    logo: ZoneEdit,
    ring: ZoneEdit,
}

pub struct App {
    shared: Shared,
    commands: Sender<Command>,
    saved: Saved,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let saved = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();

        cc.egui_ctx.global_style_mut(|style| {
            style.visuals.selection.bg_fill = ACCENT;
            style.visuals.selection.stroke.color = Color32::WHITE;
            style.spacing.item_spacing.y = 6.0;
        });

        let shared = Shared::default();
        let (commands, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let worker_state = shared.clone();
        std::thread::Builder::new()
            .name("headset".into())
            .spawn(move || device::run(worker_state, rx, move || ctx.request_repaint()))
            .expect("failed to start the headset thread");

        Self { shared, commands, saved }
    }

    fn send(&self, cmd: Command) {
        // Only fails once the worker has exited, i.e. while shutting down.
        let _ = self.commands.send(cmd);
    }

    fn draw(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        header(ui, snap);
        if let Some(err) = &snap.error {
            ui.colored_label(WARN, err);
        }
        ui.separator();

        let ready = snap.link == Link::Ready;
        // One write at a time: the worker runs commands in order, and letting
        // clicks queue up behind a slow one would replay them later.
        let enabled = ready && snap.pending.is_none();

        ui.add_enabled_ui(enabled, |ui| {
            self.noise_control(ui, snap);
            ui.add_space(8.0);
            self.sidetone(ui, snap);
            ui.add_space(8.0);
            readouts(ui, snap);
            ui.separator();
            self.lights(ui, snap);
        });

        ui.add_space(8.0);
        device_info(ui, snap);
    }

    fn noise_control(&self, ui: &mut egui::Ui, snap: &Snapshot) {
        section(ui, "Noise control", matches!(snap.pending, Some(Pending::Anc(_))));
        let pending = match snap.pending {
            Some(Pending::Anc(m)) => Some(m),
            _ => None,
        };
        let options = [(Anc::Off, "Off"), (Anc::On, "ANC"), (Anc::TalkThru, "TalkThru")];
        if let Some(mode) = segmented(ui, pending.or(snap.anc), &options) {
            self.send(Command::Anc(mode));
        }
        ui.label(
            RichText::new(match pending.or(snap.anc) {
                Some(Anc::On) => "Cancels outside noise.",
                Some(Anc::TalkThru) => "Speakers off; your surroundings are passed through.",
                Some(Anc::Off) => "No noise processing.",
                None => "",
            })
            .weak(),
        );
    }

    fn sidetone(&self, ui: &mut egui::Ui, snap: &Snapshot) {
        section(ui, "Sidetone", matches!(snap.pending, Some(Pending::Sidetone(_))));
        let pending = match snap.pending {
            Some(Pending::Sidetone(l)) => Some(l),
            _ => None,
        };
        let options = [
            (Sidetone::Off, "Off"),
            (Sidetone::Low, "Low"),
            (Sidetone::Mid, "Mid"),
            (Sidetone::High, "High"),
        ];
        if let Some(level) = segmented(ui, pending.or(snap.sidetone), &options) {
            self.send(Command::Sidetone(level));
        }
        ui.label(RichText::new("How much of your own voice you hear.").weak());
    }

    fn lights(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let busy = matches!(snap.pending, Some(Pending::Lights(_) | Pending::Lighting));
        section(ui, "Lights", busy);

        let shown = match snap.pending {
            Some(Pending::Lights(on)) => Some(on),
            _ => snap.lights_on,
        };
        if let Some(on) = segmented(ui, shown, &[(false, "Off"), (true, "On")]) {
            self.send(Command::Lights(on));
        }

        egui::CollapsingHeader::new("Colours and effects")
            .id_salt("lighting")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "The headset cannot report its lighting, so this shows what was last \
                         applied from here. Left and right earcups always match.",
                    )
                    .weak(),
                );
                zone_editor(ui, "Logo", &mut self.saved.logo);
                zone_editor(ui, "Ring", &mut self.saved.ring);
                ui.add_space(4.0);
                if ui
                    .button("Apply")
                    .on_hover_text("Switches the lights off, sends the colours, and switches them on")
                    .clicked()
                {
                    self.send(Command::Lighting(vec![
                        to_lighting(Zone::Logo, &self.saved.logo),
                        to_lighting(Zone::Ring, &self.saved.ring),
                    ]));
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let snap = self.shared.lock().unwrap().clone();
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.draw(ui, &snap));
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.saved);
    }
}

fn header(ui: &mut egui::Ui, snap: &Snapshot) {
    ui.heading("JBL Quantum 810");
    match &snap.link {
        Link::NoDongle => {
            ui.label("Plug in the headset's USB dongle.");
        }
        Link::NoAccess(msg) => {
            ui.colored_label(WARN, msg);
        }
        Link::HeadsetOff => {
            ui.label("Dongle found — switch the headset on.");
        }
        Link::Ready => {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(0x4c, 0xc2, 0x6a), "●");
                ui.label("Connected");
                if !snap.device_name.is_empty() {
                    ui.label(RichText::new(format!("as {}", snap.device_name)).weak());
                }
            });
            if let Some(pct) = snap.battery {
                let bar = egui::ProgressBar::new(f32::from(pct) / 100.0)
                    .text(format!("Battery {pct}%"))
                    .fill(if pct <= 15 { WARN } else { ACCENT });
                ui.add(bar);
            }
        }
    }
}

fn readouts(ui: &mut egui::Ui, snap: &Snapshot) {
    egui::Grid::new("readouts").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
        ui.label("Microphone");
        ui.label(match snap.mic_active {
            Some(true) => "Live",
            Some(false) => "Muted or boom up",
            None => "—",
        });
        ui.end_row();

        ui.label("Game / chat");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Chat").weak());
            // Read-only: the dial is a physical control with no write.
            let mut pos = snap.dial.unwrap_or(8);
            ui.add_enabled(false, egui::Slider::new(&mut pos, 0..=16).show_value(false));
            ui.label(RichText::new("Game").weak());
        });
        ui.end_row();
    });
}

fn device_info(ui: &mut egui::Ui, snap: &Snapshot) {
    if snap.serial.is_empty() {
        return;
    }
    egui::CollapsingHeader::new("Device info").id_salt("info").show(ui, |ui| {
        egui::Grid::new("info_grid").num_columns(2).show(ui, |ui| {
            ui.label("Serial");
            ui.label(&snap.serial);
            ui.end_row();
            ui.label("Firmware");
            ui.label(snap.firmware.join(", "));
            ui.end_row();
        });
    });
}

fn section(ui: &mut egui::Ui, title: &str, busy: bool) {
    ui.horizontal(|ui| {
        ui.strong(title);
        if busy {
            ui.spinner();
        }
    });
}

/// A row of mutually exclusive buttons. Returns the option clicked, if it
/// differs from `current`.
fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: Option<T>,
    options: &[(T, &str)],
) -> Option<T> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        for &(value, label) in options {
            let selected = current == Some(value);
            let button = egui::Button::selectable(selected, label).min_size(egui::vec2(72.0, 28.0));
            if ui.add(button).clicked() && !selected {
                chosen = Some(value);
            }
        }
    });
    chosen
}

fn zone_editor(ui: &mut egui::Ui, title: &str, zone: &mut ZoneEdit) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.strong(title);
        egui::Grid::new(title).num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            ui.label("Brightness");
            ui.add(egui::Slider::new(&mut zone.brightness, 0..=100).suffix("%"));
            ui.end_row();

            ui.label("Effect");
            egui::ComboBox::from_id_salt((title, "effect"))
                .selected_text(effect_label(Effect(zone.effect)))
                .show_ui(ui, |ui| {
                    for (effect, _) in Effect::KNOWN {
                        ui.selectable_value(&mut zone.effect, effect.0, effect_label(effect));
                    }
                });
            ui.end_row();

            ui.label("Colours");
            ui.horizontal(|ui| {
                for colour in &mut zone.colours {
                    ui.color_edit_button_srgb(colour);
                }
                if zone.colours.len() > 1 && ui.small_button("−").on_hover_text("Remove the last colour").clicked() {
                    zone.colours.pop();
                }
                if zone.colours.len() < MAX_SLOTS
                    && ui.small_button("+").on_hover_text("Add a colour to cycle through").clicked()
                {
                    let last = *zone.colours.last().unwrap_or(&[0xff, 0xff, 0xff]);
                    zone.colours.push(last);
                }
            });
            ui.end_row();
        });
    });
}

fn effect_label(effect: Effect) -> String {
    match effect.name() {
        Some(name) => {
            let words = name.replace('-', " ");
            let mut chars = words.chars();
            chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        }
        None => format!("Effect 0x{:02X}", effect.0),
    }
}

fn to_lighting(zone: Zone, edit: &ZoneEdit) -> ZoneLighting {
    ZoneLighting {
        zone,
        brightness: edit.brightness.min(100),
        palette: edit.colours.iter().map(|&[r, g, b]| Colour::new(r, g, b)).collect(),
        effect: Effect(edit.effect),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_labels_read_as_words() {
        assert_eq!(effect_label(Effect::CHASE_STROBE_LOGO), "Chase strobe logo");
        assert_eq!(effect_label(Effect(0x06)), "Effect 0x06");
    }

    #[test]
    fn saved_lighting_converts_to_a_valid_request() {
        let edit = ZoneEdit { brightness: 250, colours: vec![[1, 2, 3], [4, 5, 6]], effect: 2 };
        let l = to_lighting(Zone::Ring, &edit);
        assert_eq!(l.brightness, 100);
        assert_eq!(l.palette, vec![Colour::new(1, 2, 3), Colour::new(4, 5, 6)]);
        assert_eq!(l.effect, Effect::CHASE);
    }
}
