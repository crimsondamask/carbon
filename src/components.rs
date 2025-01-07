use crate::mutex_data::MutexData;
use egui::{
    style::Selection, Button, Color32, ComboBox, Label, PointerButton, RichText, Rounding, Slider,
    Stroke, Ui, Vec2, Visuals,
};
use egui_phosphor;
use epaint::Pos2;
pub enum Component {
    AnalogSensor(AnalogSensorConfig),
    DigitalSensor,
    Button,
    ToggleSwitch,
}

/// Analog sensor widget that displays the value of a sensor with a tag.
pub struct AnalogSensorConfig {
    pub id: usize,
    pub tag: String,
    pub value: f32,
    pub unit: String,
    pub setpoint_hh: f32,
    pub setpoint_ll: f32,
    pub alarm_hh: bool,
    pub alarm_ll: bool,
    pub pos: Pos2,
}

pub fn render_component(ui: &mut Ui, component: &mut Component, edit: bool) {
    match component {
        Component::AnalogSensor(config) => {
            let tag = ui.put(
                egui::Rect {
                    min: Pos2::new(config.pos.x, config.pos.y - 40.),
                    max: Pos2::new(config.pos.x + 150., config.pos.y + 30.),
                },
                Label::new(
                    RichText::new(format!("   {}   ", config.tag))
                        .size(14.)
                        .strong()
                        .color(Color32::BLACK)
                        .background_color(Color32::GRAY),
                )
                .sense(egui::Sense {
                    click: true,
                    drag: true,
                    focusable: true,
                }),
            );
            let widget = ui.put(
                egui::Rect {
                    min: config.pos,
                    max: Pos2::new(config.pos.x + 150., config.pos.y + 30.),
                },
                Label::new(
                    RichText::new(format!("  {:.02}  {}   ", config.value, config.unit))
                        .size(14.)
                        .strong()
                        .color(Color32::WHITE)
                        .background_color(Color32::BLACK),
                )
                .sense(egui::Sense {
                    click: true,
                    drag: true,
                    focusable: true,
                }),
            );

            if edit && tag.dragged() {
                let delta = tag.drag_delta();
                config.pos.x += delta.x;
                config.pos.y += delta.y;
            }
        }
        Component::DigitalSensor => {}
        Component::Button => {}
        Component::ToggleSwitch => {}
    }
}

pub fn update_component(component: Component, mutex_data: MutexData) {}

pub fn edit_component(component: Component) {}
