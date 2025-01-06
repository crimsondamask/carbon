use egui::{
    style::Selection, Button, Color32, ComboBox, Label, PointerButton, RichText, Rounding, Slider,
    Stroke, Ui, Vec2, Visuals,
};
use egui_phosphor;
use epaint::Pos2;
use crate::mutex_data::MutexData;
pub enum Component {
    AnalogSensor(AnalogSensorConfig),
    DigitalSensor,
    Button,
    ToggleSwitch,
}

/// Analog sensor widget that displays the value of a sensor with a tag.
pub struct AnalogSensorConfig {
    pub tag: String,
    pub value: f32,
    pub setpoint_hh: f32,
    pub setpoint_ll: f32,
    pub alarm_hh: bool,
    pub alarm_ll: bool,
    pub pos: Pos2,
}


pub fn render_component(ui: &mut Ui, component: Component) {
    match component {
        Component::AnalogSensor(config) => {}
        Component::DigitalSensor => {}
        Component::Button => {}
        Component::ToggleSwitch => {}
    }
}

pub fn update_component(component: Component, mutex_data: MutexData) {

}

pub fn edit_component(component: Component) {}