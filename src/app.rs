use egui::{
    style::Selection, Button, Color32, ComboBox, Label, PointerButton, RichText, Rounding, Slider,
    Stroke, Vec2, Visuals,
};

use egui_phosphor;
use epaint::Pos2;
use parking_lot::Mutex;
use rfd;
use rmodbus::{client::ModbusRequest, ModbusProto};
use serialport::available_ports;
use std::path::PathBuf;
use std::{
    fmt::Display,
    fs::File,
    net::SocketAddr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use sync::tcp::{connect_slave_with_timeout, connect_with_timeout};
use tokio_modbus::prelude::{sync::rtu::connect_slave, sync::tcp::connect, *};

use actix_web::{middleware, rt, web, App, HttpRequest, HttpServer};

use s7::{client::Client, field::Bool, field::Fields, field::Float, tcp, transport::Connection};
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::net::{IpAddr, Ipv4Addr};

//#################################################### Main App Struct

const DB: i32 = 1;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct CarbonApp {
    #[serde(skip)]
    app_run_state: AppRunState,
    device_config_buffer: DeviceConfigUiBuffer,
    #[serde(skip)]
    mutex: Arc<Mutex<MutexData>>,
    protocol: Protocol,
    #[serde(skip)]
    device_config: DeviceConfig,
    #[serde(skip)]
    tag1: f32,
    tag2: f32,
    tag3: f32,
    #[serde(skip)]
    about: bool,
    #[serde(skip)]
    options: bool,
    #[serde(skip)]
    edit_pos: bool,
    #[serde(skip)]
    tags: Vec<Tag>,
    #[serde(skip)]
    digital_inputs: u16,
    #[serde(skip)]
    digital_inputs2: u16,
    widgets_pos: WidgetsPos,
    #[serde(skip)]
    blink_time: usize,
    blink_flag: bool,
    logger_path: PathBuf,
    setpoint_buffer: String,
    esd_status: bool,
    reset_status: bool,
}
//####################################################

//#################################################### The Mutex used between
//the main and background threads.
struct MutexData {
    data: Vec<u16>,
    s7_read_data: S7Data,
    s7_message: Option<S7MessageTag>,
    modbus_float_message: Option<ModbusFloatInput>,
    modbus_bool_message: Option<ModbusBoolInput>,
    achieved_scan_time: u128,
    error_msg: String,
    new_config: Option<DeviceConfigUiBuffer>,
    log: bool,
    kill_thread: bool,
}
//####################################################

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
struct Tag {
    name: String,
    #[serde(skip)]
    value: f32,
    pos: Pos2,
}
#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
struct WidgetsPos {
    hello_button_pos: Pos2,
    close_button_pos: Pos2,
    tag1_pos: Pos2,
}
//#################################################### The available protocols.
#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum Protocol {
    ModbusTcpProtocol,
    ModbusRtuProtocol,
    EthernetIpProtocol,
    S7Protocol,
    Datascan,
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::ModbusTcpProtocol => write!(f, "Modbus TCP"),
            Protocol::ModbusRtuProtocol => write!(f, "Modbus Serial"),
            Protocol::EthernetIpProtocol => write!(f, "Ethernet/IP"),
            Protocol::S7Protocol => write!(f, "Siemens S7"),
            Protocol::Datascan => write!(f, "Datascan"),
        }
    }
}

impl Default for Protocol {
    fn default() -> Self {
        Self::ModbusRtuProtocol
    }
}
//####################################################

// ################################################### Helper structs

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct AppRunState {
    is_loop_running: bool,
    is_ui_apply_clicked: bool,
    enable_proto_opt_edit: bool,
    enable_device_opt_edit: bool,
}

impl Default for AppRunState {
    fn default() -> Self {
        Self {
            is_loop_running: false,
            is_ui_apply_clicked: false,
            enable_proto_opt_edit: true,
            enable_device_opt_edit: true,
        }
    }
}
#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct DeviceConfigUiBuffer {
    modbus_serial_buffer: ModbusSerialConfig,
    modbus_tcp_buffer: ModbusTcpConfig,
    ethernet_ip_buffer: EthernetIpConfig,
    s7_buffer: S7Config,
}

impl Default for DeviceConfigUiBuffer {
    fn default() -> Self {
        Self {
            modbus_serial_buffer: ModbusSerialConfig::default(),
            modbus_tcp_buffer: ModbusTcpConfig::default(),
            ethernet_ip_buffer: EthernetIpConfig,
            s7_buffer: S7Config::default(),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum RegisterType {
    Coils,
    Inputs,
    Holding,
}

impl Display for RegisterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterType::Coils => write!(f, "Coils"),
            RegisterType::Inputs => write!(f, "Input registers"),
            RegisterType::Holding => write!(f, "Holding registers"),
        }
    }
}

impl Default for RegisterType {
    fn default() -> Self {
        Self::Holding
    }
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum Parity {
    Even,
    Odd,
    NoneParity,
}

impl Display for Parity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Parity::Even => write!(f, "Even"),
            Parity::Odd => write!(f, "Odd"),
            Parity::NoneParity => write!(f, "None"),
        }
    }
}

impl Default for Parity {
    fn default() -> Self {
        Self::NoneParity
    }
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum Baudrate {
    Baud38400,
    Baud9600,
}

impl Display for Baudrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Baudrate::Baud38400 => write!(f, "38400"),
            Baudrate::Baud9600 => write!(f, "9600"),
        }
    }
}

impl Default for Baudrate {
    fn default() -> Self {
        Self::Baud38400
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct ModbusSerialConfig {
    port: String,
    baudrate: Baudrate,
    slave: u8,
    slave_buffer: String,
    parity: Parity,
    protocol_definitions: ModbusDefinitions,
}

impl Default for ModbusSerialConfig {
    fn default() -> Self {
        Self {
            port: "".to_string(),
            baudrate: Baudrate::default(),
            slave: 1,
            slave_buffer: "1".to_string(),
            parity: Parity::default(),
            protocol_definitions: ModbusDefinitions::default(),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct ModbusTcpConfig {
    ip_address: String,
    port: usize,
    protocol_definitions: ModbusDefinitions,
}

impl Default for ModbusTcpConfig {
    fn default() -> Self {
        Self {
            ip_address: "192.168.0.1".to_string(),
            port: 502,
            protocol_definitions: ModbusDefinitions::default(),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct EthernetIpConfig;

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct S7Config {
    ip: String,
}

impl Default for S7Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct S7Data {
    tag1: f32,
    tag2: f32,
    tag3: f32,
    tag4: bool,
    tag5: bool,
    tag6: bool,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
struct ModbusFloatInput {
    register: u16,
    value: f32,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
struct ModbusBoolInput {
    register: u16,
    value: bool,
}
#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct S7MessageTag {
    message: S7Message,
    db: i32,
    offset: f32,
}
#[derive(serde::Deserialize, serde::Serialize, Clone)]
enum S7Message {
    S7Bool(bool),
    S7Real(f32),
    S7Int(i16),
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
struct ModbusDefinitions {
    register_type: RegisterType,
    start_address: u16,
    register_count: u16,
    scan_delay: u64,
    request_function_vec: Vec<u8>,
}

impl Default for ModbusDefinitions {
    fn default() -> Self {
        Self {
            register_type: RegisterType::default(),
            start_address: 0,
            register_count: 44,
            scan_delay: 1000,
            request_function_vec: Vec::with_capacity(32),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
enum DeviceConfig {
    ModbusTcp(ModbusTcpConfig),
    ModbusSerial(ModbusSerialConfig),
    EthernetIp(EthernetIpConfig),
    S7(S7Config),
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self::ModbusSerial(ModbusSerialConfig::default())
    }
}

// ###################################################

impl Default for CarbonApp {
    fn default() -> Self {
        let mut tags: Vec<Tag> = Vec::new();

        tags.push(Tag {
            name: "FLP".to_string(),
            value: 0.0,
            pos: Pos2 { x: 320., y: 380. },
        });
        tags.push(Tag {
            name: "FLT".to_string(),
            value: 0.0,
            pos: Pos2 { x: 184., y: 380. },
        });
        tags.push(Tag {
            name: "FLR".to_string(),
            value: 0.0,
            pos: Pos2 { x: 320., y: 220. },
        });
        tags.push(Tag {
            name: "HPP".to_string(),
            value: 0.0,
            pos: Pos2 { x: 440., y: 220. },
        });
        tags.push(Tag {
            name: "CH T1".to_string(),
            value: 0.0,
            pos: Pos2 { x: 450., y: 560. },
        });
        tags.push(Tag {
            name: "CH T2".to_string(),
            value: 0.0,
            pos: Pos2 { x: 580., y: 560. },
        });
        tags.push(Tag {
            name: "CH T3".to_string(),
            value: 0.0,
            pos: Pos2 { x: 820., y: 380. },
        });
        tags.push(Tag {
            name: "CH T4".to_string(),
            value: 0.0,
            pos: Pos2 { x: 820., y: 560. },
        });
        tags.push(Tag {
            name: "CH S1".to_string(),
            value: 0.0,
            pos: Pos2 { x: 580., y: 380. },
        });
        tags.push(Tag {
            name: "CH S2".to_string(),
            value: 0.0,
            pos: Pos2 { x: 320., y: 560. },
        });
        tags.push(Tag {
            name: "CH S3".to_string(),
            value: 0.0,
            pos: Pos2 { x: 700., y: 380. },
        });
        tags.push(Tag {
            name: "CH S4".to_string(),
            value: 0.0,
            pos: Pos2 { x: 700., y: 560. },
        });
        tags.push(Tag {
            name: "BP P".to_string(),
            value: 0.0,
            pos: Pos2 { x: 450., y: 380. },
        });
        tags.push(Tag {
            name: "DTL".to_string(),
            value: 0.0,
            pos: Pos2 { x: 36., y: 380. },
        });
        tags.push(Tag {
            name: "FUDP".to_string(),
            value: 0.0,
            pos: Pos2 { x: 580., y: 220. },
        });
        tags.push(Tag {
            name: "WTL".to_string(),
            value: 0.0,
            pos: Pos2 { x: 190., y: 220. },
        });
        tags.push(Tag {
            name: "Pressure Setpoint".to_string(),
            value: 0.0,
            pos: Pos2 { x: 200., y: 300. },
        });
        tags.push(Tag {
            name: "ESD Output".to_string(),
            value: 0.0,
            pos: Pos2 { x: 200., y: 300. },
        });
        tags.push(Tag {
            name: "RESET Output".to_string(),
            value: 0.0,
            pos: Pos2 { x: 200., y: 300. },
        });
        tags.push(Tag {
            name: "EV1-1".to_string(),
            value: 0.0,
            pos: Pos2 { x: 800., y: 100. },
        });
        tags.push(Tag {
            name: "EV1-2".to_string(),
            value: 0.0,
            pos: Pos2 { x: 920., y: 100. },
        });
        tags.push(Tag {
            name: "EV1-3".to_string(),
            value: 0.0,
            pos: Pos2 { x: 1040., y: 100. },
        });

        Self {
            // Example stuff:
            app_run_state: AppRunState::default(),
            device_config_buffer: DeviceConfigUiBuffer::default(),
            mutex: Arc::new(Mutex::new(MutexData {
                data: Vec::new(),
                s7_read_data: S7Data {
                    tag1: 0.0,
                    tag2: 0.0,
                    tag3: 0.0,
                    tag4: false,
                    tag5: false,
                    tag6: false,
                },
                s7_message: None,
                modbus_float_message: None,
                modbus_bool_message: None,
                achieved_scan_time: 0,
                error_msg: "".to_string(),
                new_config: None,
                log: true,
                kill_thread: false,
            })),
            protocol: Protocol::default(),
            device_config: DeviceConfig::default(),
            tag1: 0.0,
            tag2: 0.0,
            tag3: 0.0,
            about: false,
            options: false,
            edit_pos: false,
            widgets_pos: WidgetsPos {
                hello_button_pos: Pos2::new(850., 350.),
                close_button_pos: Pos2::new(1050., 350.),
                tag1_pos: Pos2::new(450., 350.),
            },
            tags,
            digital_inputs: 0,
            digital_inputs2: 0,
            blink_time: 1000,
            blink_flag: false,
            logger_path: PathBuf::from("./LOGGER.txt"),
            setpoint_buffer: "50.0".to_owned(),
            esd_status: false,
            reset_status: false,
        }
    }
}

impl CarbonApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "custom_font".to_owned(),
            egui::FontData::from_static(include_bytes!("../assets/plex.ttf")),
            //egui::FontData::from_static(include_bytes!("../assets/dejavu.ttf")),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "custom_font".to_owned());

        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::variants::Variant::Regular);

        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.set_fonts(fonts);

        // Configuring visuals.

        let mut visuals = Visuals::light();
        visuals.selection = Selection {
            bg_fill: Color32::from_rgb(81, 129, 154),
            stroke: Stroke::new(1.0, Color32::WHITE),
        };

        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(180, 180, 180);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(180, 180, 180);
        visuals.widgets.inactive.rounding = Rounding::ZERO;
        visuals.widgets.noninteractive.rounding = Rounding::ZERO;
        visuals.widgets.active.rounding = Rounding::ZERO;
        visuals.widgets.hovered.rounding = Rounding::ZERO;
        visuals.window_rounding = Rounding::ZERO;
        visuals.window_fill = Color32::from_rgb(197, 197, 197);
        visuals.menu_rounding = Rounding::ZERO;
        visuals.panel_fill = Color32::from_rgb(200, 200, 200);
        visuals.striped = true;
        visuals.slider_trailing_fill = true;

        cc.egui_ctx.set_visuals(visuals);

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }
}

impl eframe::App for CarbonApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Self {
            app_run_state,
            device_config_buffer,
            mutex,
            protocol,
            device_config,
            tag1,
            tag2,
            tag3,
            about,
            options,
            edit_pos,
            widgets_pos,
            tags,
            digital_inputs,
            digital_inputs2,
            blink_time,
            blink_flag,
            logger_path,
            setpoint_buffer,
            esd_status,
            reset_status,
        } = self;

        ctx.request_repaint();
        #[cfg(not(target_arch = "wasm32"))] // no File->Quit on web pages!
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Options").clicked() {
                        *options = !*options;
                    }
                    if ui.button("Quit").clicked() {
                        _frame.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    ui.checkbox(edit_pos, "Edit positions");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        *about = !*about;
                    }
                });
            });
        });

        egui::Window::new("About").open(about).show(ctx, |ui| {
            ui.add(Label::new(RichText::new(
                "Developed by Abdelkader Madoui. All rights reserved 2024.\nabdelkadermadoui@protonmail.com",
            )));
        });
        egui::TopBottomPanel::bottom("bottom-panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 30.0;
                    ui.colored_label(Color32::GRAY, "Carbon v0.1");

                    if let Some(data) = mutex.try_lock() {
                        let achieved_scan_time = data.achieved_scan_time;
                        let error_msg = &data.error_msg;
                        ui.colored_label(
                            Color32::GRAY,
                            format!("Achieved scan time: {} μs", achieved_scan_time),
                        );
                        ui.colored_label(Color32::GRAY, format!("{}", error_msg));
                    }
                });
            });
        });

        egui::Window::new("Options").open(options).show(ctx, |ui| {
            ui.label(format!(
                "{} Protocol Configuration",
                egui_phosphor::regular::GEAR_SIX
            ));

            ui.horizontal(|ui| {
                ComboBox::from_label("Protocol")
                    .selected_text(format!("{}", protocol))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(protocol, Protocol::ModbusTcpProtocol, "Modbus TCP");
                        ui.selectable_value(protocol, Protocol::ModbusRtuProtocol, "Modbus Serial");
                        ui.selectable_value(protocol, Protocol::EthernetIpProtocol, "EthernetIP");
                        ui.selectable_value(protocol, Protocol::S7Protocol, "Siemens S7");
                        ui.selectable_value(protocol, Protocol::Datascan, "Datascan");
                    });
                match protocol {
                    Protocol::ModbusTcpProtocol => {
                        ui.image(egui::include_image!("../assets/modbus-logo.png"));
                    }
                    Protocol::ModbusRtuProtocol => {
                        ui.image(egui::include_image!("../assets/modbus-logo.png"));
                    }

                    Protocol::EthernetIpProtocol => {
                        ui.image(egui::include_image!("../assets/ethernet-ip-logo.jpg"));
                    }
                    Protocol::S7Protocol => {
                        ui.image(egui::include_image!("../assets/siemens-logo.jpg"));
                    }
                    Protocol::Datascan => {}
                }
            });

            match protocol {
                Protocol::EthernetIpProtocol => {}
                Protocol::Datascan => {}
                Protocol::S7Protocol => {
                    s7_device_ui(ui, device_config_buffer);
                    *device_config = DeviceConfig::S7(device_config_buffer.s7_buffer.clone());
                }
                Protocol::ModbusTcpProtocol => {
                    // Modbus TCP UI
                    // Switch to ModbusTcpConfig
                    ui.group(|ui| {
                        ui.set_enabled(app_run_state.enable_device_opt_edit);
                        ui.label(format!("{} Device Options", egui_phosphor::regular::WRENCH));

                        modbus_tcp_device_ui(ui, device_config_buffer);
                    });
                    ui.separator();
                    ui.group(|ui| {
                        ui.set_enabled(
                            app_run_state.is_ui_apply_clicked || !app_run_state.is_loop_running,
                        );
                        ui.label(format!(
                            "{} Request Options",
                            egui_phosphor::regular::WRENCH
                        ));

                        modbus_protocol_ui(
                            &mut device_config_buffer.modbus_tcp_buffer.protocol_definitions,
                            ui,
                        );
                    });

                    ui.group(|ui| {
                        ui.set_enabled(false);
                        modbus_request_details_ui(
                            ui,
                            &mut device_config_buffer.modbus_tcp_buffer.protocol_definitions,
                        );
                    });
                    *device_config =
                        DeviceConfig::ModbusTcp(device_config_buffer.modbus_tcp_buffer.clone());
                }
                Protocol::ModbusRtuProtocol => {
                    // Modbus serial UI
                    // Switch to ModbusSerialConfig
                    ui.group(|ui| {
                        ui.set_enabled(app_run_state.enable_device_opt_edit);
                        ui.label(format!("{} Device Options", egui_phosphor::regular::WRENCH));

                        modbus_serial_device_ui(device_config_buffer, ui);
                    });
                    ui.separator();
                    ui.group(|ui| {
                        ui.set_enabled(
                            app_run_state.is_ui_apply_clicked || !app_run_state.is_loop_running,
                        );
                        ui.label(format!(
                            "{} Request Options",
                            egui_phosphor::regular::WRENCH
                        ));

                        modbus_protocol_ui(
                            &mut device_config_buffer
                                .modbus_serial_buffer
                                .protocol_definitions,
                            ui,
                        );
                    });

                    ui.group(|ui| {
                        ui.set_enabled(false);
                        modbus_request_details_ui(
                            ui,
                            &mut device_config_buffer
                                .modbus_serial_buffer
                                .protocol_definitions,
                        );
                    });
                    *device_config = DeviceConfig::ModbusSerial(
                        device_config_buffer.modbus_serial_buffer.clone(),
                    );
                }
            }

            ui.separator();
            egui::Grid::new("Buttons")
                .num_columns(3)
                .min_col_width(100.)
                .show(ui, |ui| {
                    if ui
                        .add_enabled(
                            !app_run_state.is_loop_running,
                            Button::new(format!("Connect")).min_size(Vec2::new(100., 10.)),
                        )
                        //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                        .clicked()
                    {
                        app_run_state.enable_device_opt_edit = false;
                        app_run_state.enable_proto_opt_edit = true;
                        app_run_state.is_loop_running = true;
                        let mutex = Arc::clone(&mutex);

                        let res = rfd::FileDialog::new()
                            .set_file_name("LOGGER.txt")
                            .set_directory(&logger_path.parent().unwrap())
                            .set_can_create_directories(true)
                            .save_file();
                        // Spawn the data polling thread.
                        if let Some(path) = res {
                            *logger_path = path;
                            spawn_polling_thread(device_config, mutex, &logger_path);
                        }

                        thread::spawn(move || {
                            let server_future = run_app();
                            rt::System::new().block_on(server_future)
                        });
                    }
                    if !app_run_state.is_ui_apply_clicked {
                    } else {
                        if ui
                            .add_enabled(
                                app_run_state.enable_proto_opt_edit
                                    && app_run_state.is_loop_running,
                                Button::new(format!("{} Apply", egui_phosphor::regular::PEN)),
                            )
                            //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                            .clicked()
                        {
                            let mutex = Arc::clone(&mutex);
                            mutex.lock().new_config = Some(device_config_buffer.clone());
                            app_run_state.is_ui_apply_clicked = false;
                        }
                    }
                });
        });
        egui::SidePanel::right("right_panel")
            .resizable(false)
            .default_width(130.)
            .min_width(130.)
            .show(ctx, |ui| {
                //ui.image(egui::include_image!("../assets/lours.png")).max_width(40.);
                ui.add(egui::Image::new(egui::include_image!(
                    "../assets/lours.png"
                )));

                ui.separator();
                ui.separator();
                ui.label("Pressure Setpoint:");
                ui.add(egui::TextEdit::singleline(setpoint_buffer).desired_width(120.));
                if ui.button("Write").clicked() {
                    let value = setpoint_buffer.parse::<f32>();
                    if let Ok(value) = value {
                        if let Some(mut data) = mutex.try_lock() {
                            data.modbus_float_message = Some(ModbusFloatInput {
                                register: 32,
                                value,
                            });
                        }
                    }
                }
                /*
                    ui.vertical(|ui| {
                        digital_values(ui, *digital_inputs, 0, "ESD PUSH BUTTON".to_string());
                        digital_values(ui, *digital_inputs, 1, "TANK LVL 10%".to_string());
                        digital_values(ui, *digital_inputs, 2, "TANK LVL 5%".to_string());
                        digital_values(ui, *digital_inputs, 3, "PT3-1 LOW".to_string());
                        digital_values(ui, *digital_inputs, 4, "HP1-1 MTNCE REQ".to_string());
                        digital_values(ui, *digital_inputs, 5, "REGU FAULT HP".to_string());
                        digital_values(ui, *digital_inputs, 6, "SCSSV PRES LOW".to_string());
                        digital_values(ui, *digital_inputs, 7, "MV PRES LOW".to_string());
                        digital_values(ui, *digital_inputs, 8, "ESDV PRES LOW".to_string());
                        digital_values(ui, *digital_inputs, 9, "PT1-1 PRES HIGH".to_string());
                        digital_values(ui, *digital_inputs, 10, "PLC-1 COM FAIL".to_string());
                        digital_values(ui, *digital_inputs, 11, "PLC-2 COM FAIL".to_string());
                        digital_values(ui, *digital_inputs2, 0, "ESD-1 FIRE EMG".to_string());
                        digital_values(ui, *digital_inputs2, 1, "ESD-3 SHUTDOWN".to_string());
                        digital_values(ui, *digital_inputs2, 2, "DIESEL LVL".to_string());
                        digital_values(ui, *digital_inputs2, 3, "WI PUMP OFF".to_string());
                        digital_values(ui, *digital_inputs2, 4, "WATER PUMP TEMP".to_string());
                        digital_values(ui, *digital_inputs2, 5, "WATER TNK LVL".to_string());
                        digital_values(ui, *digital_inputs2, 6, "CHEMICAL TNK LVL1".to_string());
                        digital_values(ui, *digital_inputs2, 7, "CHEMICAL TNK LVL2".to_string());
                        digital_values(ui, *digital_inputs2, 8, "CHEMICAL TNK LVL3".to_string());
                        digital_values(ui, *digital_inputs2, 9, "CHEMICAL TNK LVL4".to_string());
                        digital_values(ui, *digital_inputs2, 10, "DIFF PRES FILTRATION".to_string());
                        digital_values(ui, *digital_inputs2, 11, "HIGH PRES FLOWLINE".to_string());
                        digital_values(ui, *digital_inputs2, 12, "LOW PRES FLOWLINE".to_string());
                        digital_values(ui, *digital_inputs2, 14, "UNHEALTHY RESET".to_string());
                    });
                */
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.separator();
            {
                if let Some(data) = mutex.try_lock() {
                    if data.data.len() > 36 {
                        tags[0].value = u16_to_float(data.data[0], data.data[1]);
                        tags[1].value = u16_to_float(data.data[2], data.data[3]);
                        tags[2].value = u16_to_float(data.data[4], data.data[5]);
                        tags[3].value = u16_to_float(data.data[6], data.data[7]);
                        tags[4].value = u16_to_float(data.data[8], data.data[9]);
                        tags[5].value = u16_to_float(data.data[10], data.data[11]);
                        tags[6].value = u16_to_float(data.data[12], data.data[13]);
                        tags[7].value = u16_to_float(data.data[14], data.data[15]);
                        tags[8].value = u16_to_float(data.data[16], data.data[17]);
                        tags[9].value = u16_to_float(data.data[18], data.data[19]);
                        tags[10].value = u16_to_float(data.data[20], data.data[21]);
                        tags[11].value = u16_to_float(data.data[22], data.data[23]);
                        tags[12].value = u16_to_float(data.data[24], data.data[25]);
                        tags[13].value = u16_to_float(data.data[26], data.data[27]);
                        tags[14].value = u16_to_float(data.data[28], data.data[29]);
                        tags[15].value = u16_to_float(data.data[30], data.data[31]);
                        tags[16].value = u16_to_float(data.data[32], data.data[33]);
                        tags[17].value = u16_to_float(data.data[34], data.data[35]);
                        tags[18].value = u16_to_float(data.data[36], data.data[37]);
                        tags[19].value = u16_to_float(data.data[38], data.data[39]);
                        tags[20].value = u16_to_float(data.data[40], data.data[41]);
                        tags[21].value = u16_to_float(data.data[42], data.data[43]);
                        if tags[17].value >= 1.0 {
                            *reset_status = true;
                        } else {
                            *reset_status = false;
                        }
                        if tags[18].value >= 1.0 {
                            *esd_status = true;
                        } else {
                            *esd_status = false;
                        }
                    }
                    // *tag1 = data.s7_read_data.tag1;
                    // *tag2 = data.s7_read_data.tag2;
                    // *tag3 = data.s7_read_data.tag3;
                }
            }
            egui::Image::new("file://background.jpg").paint_at(ui, ui.ctx().available_rect());
            // egui::Image::new(egui::include_image!("../assets/sample.png"))
            //     .paint_at(ui, ui.ctx().available_rect());

            hello_button(ui, widgets_pos, edit_pos, mutex, *reset_status);
            close_button(ui, widgets_pos, edit_pos, mutex, *esd_status);

            // tag1_func(ui, widgets_pos, edit_pos, tag1);
            tag_func(
                ui,
                edit_pos,
                &mut tags[0],
                "Psig".to_string(),
                "Hydr Oil Lvl".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[1],
                "DegC".to_string(),
                "WHCP Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[2],
                "M3/h".to_string(),
                "MP Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[3],
                "Psig".to_string(),
                "SCSSV Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[4],
                "DegC".to_string(),
                "MV Hydr Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[5],
                "DegC".to_string(),
                "ESDV Hydr Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[6],
                "DegC".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[7],
                "DegC".to_string(),
                "ESDV Status Wtr Injection".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[8],
                "DegC".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[9],
                "DegC".to_string(),
                "ESDV Status Wtr Injection".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[10],
                "DegC".to_string(),
                "Hydr Oil Lvl".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[11],
                "DegC".to_string(),
                "WHCP Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[12],
                "Psig".to_string(),
                "MP Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[13],
                "%".to_string(),
                "SCSSV Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[14],
                "Psig".to_string(),
                "MV Hydr Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[15],
                "%".to_string(),
                "ESDV Hydr Oil Pressure".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[16],
                "Psig".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            /*
            tag_func(
                ui,
                edit_pos,
                &mut tags[17],
                "Barg".to_string(),
                "ESDV Status Wtr Injection".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[18],
                "Barg".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
             */
            sdv_tag(
                ui,
                edit_pos,
                &mut tags[19],
                "Barg".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            sdv_tag(
                ui,
                edit_pos,
                &mut tags[20],
                "Barg".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            sdv_tag(
                ui,
                edit_pos,
                &mut tags[21],
                "Barg".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            /*
            tag_func(
                ui,
                edit_pos,
                &mut tags[19],
                "Barg".to_string(),
                "ESDV Status Wtr Injection".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[20],
                "Barg".to_string(),
                "Fusible Plug Hydr Oil".to_owned(),
            );
            tag_func(
                ui,
                edit_pos,
                &mut tags[21],
                "Barg".to_string(),
                "ESDV Status Wtr Injection".to_owned(),
            );
             */
        });
    }
}

fn tag1_func(ui: &mut egui::Ui, widgets_pos: &mut WidgetsPos, edit_pos: &mut bool, tag1: &mut f32) {
    ui.put(
        egui::Rect {
            min: Pos2::new(widgets_pos.tag1_pos.x, widgets_pos.tag1_pos.y - 40.),
            max: Pos2::new(widgets_pos.tag1_pos.x + 150., widgets_pos.tag1_pos.y + 30.),
        },
        Label::new(
            RichText::new(format!("  PIT 0001  "))
                .size(14.)
                .strong()
                .color(Color32::BLACK)
                .background_color(Color32::GRAY),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );
    let tag1_widget = ui.put(
        egui::Rect {
            min: widgets_pos.tag1_pos,
            max: Pos2::new(widgets_pos.tag1_pos.x + 150., widgets_pos.tag1_pos.y + 30.),
        },
        Label::new(
            RichText::new(format!("  {:.02} barg  ", tag1))
                .size(14.)
                .strong()
                .color(Color32::WHITE)
                .background_color(Color32::BLACK),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );

    if tag1_widget.dragged() {
        let delta = tag1_widget.drag_delta();
        widgets_pos.tag1_pos.x += delta.x;
        widgets_pos.tag1_pos.y += delta.y;
    }
}

fn sdv_tag(ui: &mut egui::Ui, edit_pos: &mut bool, tag: &mut Tag, unit: String, desc: String) {
    let mut color = Color32::GRAY;

    if tag.value >= 1.0 {
        color = Color32::GREEN;
    } else {
        color = Color32::RED;
    }
    let sdv = ui.put(
        egui::Rect {
            min: Pos2::new(tag.pos.x, tag.pos.y - 40.),
            max: Pos2::new(tag.pos.x + 150., tag.pos.y + 30.),
        },
        Label::new(
            RichText::new(format!("   {}   ", tag.name))
                .size(14.)
                .strong()
                .color(Color32::BLACK)
                .background_color(color),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );

    /*
    ui.put(
        egui::Rect {
            min: Pos2::new(tag.pos.x + 30., tag.pos.y + 20.),
            max: Pos2::new(tag.pos.x + 180., tag.pos.y + 110.),
        },
        egui::Image::new(egui::include_image!("../assets/sdv.svg")),
    );
     */
    if sdv.dragged() {
        let delta = sdv.drag_delta();
        tag.pos.x += delta.x;
        tag.pos.y += delta.y;
    }
}
fn tag_func(ui: &mut egui::Ui, edit_pos: &mut bool, tag: &mut Tag, unit: String, desc: String) {
    /*
    ui.put(
        egui::Rect {
            min: Pos2::new(tag.pos.x, tag.pos.y - 45.),
            max: Pos2::new(tag.pos.x + 150., tag.pos.y + 0.),
        },
        Label::new(
            RichText::new(format!("{}", desc))
                .size(12.)
                .color(Color32::BLACK)
                .background_color(Color32::GRAY),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );
     */
    ui.put(
        egui::Rect {
            min: Pos2::new(tag.pos.x, tag.pos.y - 40.),
            max: Pos2::new(tag.pos.x + 150., tag.pos.y + 30.),
        },
        Label::new(
            RichText::new(format!("   {}   ", tag.name))
                .size(14.)
                .strong()
                .color(Color32::BLACK)
                .background_color(Color32::GRAY),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );

    let tag1_widget = ui.put(
        egui::Rect {
            min: tag.pos,
            max: Pos2::new(tag.pos.x + 150., tag.pos.y + 30.),
        },
        Label::new(
            RichText::new(format!("  {:.02}  {}   ", tag.value, unit))
                .size(14.)
                .strong()
                .color(Color32::WHITE)
                .background_color(Color32::BLACK),
        )
        .sense(egui::Sense {
            click: true,
            drag: *edit_pos,
            focusable: true,
        }),
    );
    /*
    ui.put(
        egui::Rect {
            min: Pos2::new(tag.pos.x, tag.pos.y + 30.),
            max: Pos2::new(tag.pos.x + 150., tag.pos.y + 80.),
        },
        egui::Image::new(egui::include_image!("../assets/sensor.png")),
    );
     */
    if tag1_widget.dragged() {
        let delta = tag1_widget.drag_delta();
        tag.pos.x += delta.x;
        tag.pos.y += delta.y;
    }
}
fn digital_values(ui: &mut egui::Ui, reg: u16, bit: usize, label: String) {
    if check_bit(reg, bit) {
        ui.add(Label::new(
            RichText::new(format!("  {}  ", label))
                .size(12.)
                .strong()
                .color(Color32::GRAY),
        ));
    } else {
        ui.add(Label::new(
            RichText::new(format!("  {}  ", label))
                .size(12.)
                .strong()
                .color(Color32::WHITE)
                .background_color(Color32::DARK_RED),
        ));
    }
}

fn check_bit(value: u16, n: usize) -> bool {
    if n < 16 {
        value & (1 << n) != 0
    } else {
        false
    }
}
fn hello_button(
    ui: &mut egui::Ui,
    widgets_pos: &mut WidgetsPos,
    edit_pos: &mut bool,
    mutex: &mut Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, MutexData>>,
    reset_status: bool,
) {
    let mut button_color = Color32::GRAY;

    if reset_status {
        button_color = Color32::GRAY;
    } else {
        button_color = Color32::YELLOW;
    }

    let hello_button = ui.put(
        egui::Rect {
            min: widgets_pos.hello_button_pos,
            max: Pos2::new(
                widgets_pos.hello_button_pos.x + 70.,
                widgets_pos.hello_button_pos.y + 70.,
            ),
        },
        Button::new("RESET")
            .fill(button_color)
            .rounding(60.)
            .sense(egui::Sense {
                click: !*edit_pos,
                drag: *edit_pos,
                focusable: true,
            }),
    );

    if hello_button.dragged() {
        let delta = hello_button.drag_delta();

        widgets_pos.hello_button_pos.x += delta.x;
        widgets_pos.hello_button_pos.y += delta.y;
    }

    if hello_button.clicked() {
        if let Some(mut data) = mutex.try_lock() {
            if data.data[34] > 0 {
                data.modbus_float_message = Some(ModbusFloatInput {
                    register: 34,
                    value: 0.0,
                });
            } else {
                data.modbus_float_message = Some(ModbusFloatInput {
                    register: 34,
                    value: 1.0,
                });
            }
        }
    }
}
fn close_button(
    ui: &mut egui::Ui,
    widgets_pos: &mut WidgetsPos,
    edit_pos: &mut bool,
    mutex: &mut Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, MutexData>>,
    esd_status: bool,
) {
    let mut button_color = Color32::GRAY;

    if esd_status {
        button_color = Color32::GRAY;
    } else {
        button_color = Color32::RED;
    }
    let close_button = ui.put(
        egui::Rect {
            min: widgets_pos.close_button_pos,
            max: Pos2::new(
                widgets_pos.close_button_pos.x + 70.,
                widgets_pos.close_button_pos.y + 70.,
            ),
        },
        Button::new("ESD")
            .fill(button_color)
            .rounding(60.)
            .sense(egui::Sense {
                click: !*edit_pos,
                drag: *edit_pos,
                focusable: true,
            }),
    );

    if close_button.dragged() {
        let delta = close_button.drag_delta();

        widgets_pos.close_button_pos.x += delta.x;
        widgets_pos.close_button_pos.y += delta.y;
    }

    if close_button.clicked() {
        if let Some(mut data) = mutex.try_lock() {
            if data.data[36] > 0 {
                data.modbus_float_message = Some(ModbusFloatInput {
                    register: 36,
                    value: 0.0,
                });
            } else {
                data.modbus_float_message = Some(ModbusFloatInput {
                    register: 36,
                    value: 1.0,
                });
            }
        }
    }
}

fn modbus_request_details_ui(
    ui: &mut egui::Ui,
    modbus_protocol_definitions: &mut ModbusDefinitions,
) -> egui::InnerResponse<()> {
    ui.horizontal(|ui| {
        let mut modbus_request_vec = Vec::new();
        ui.label("Request:");
        let mut modbus_request = ModbusRequest::new(1, ModbusProto::TcpUdp);
        match modbus_protocol_definitions.register_type {
            RegisterType::Holding => {
                if modbus_request
                    .generate_get_holdings(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
            RegisterType::Inputs => {
                if modbus_request
                    .generate_get_inputs(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
            RegisterType::Coils => {
                if modbus_request
                    .generate_get_coils(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
        }
        for i in 0..modbus_request_vec.len() {
            ui.label(format!("{:02X}", modbus_request_vec[i]));
        }
        modbus_protocol_definitions.request_function_vec = modbus_request_vec;
    })
}

fn _modbus_serial_request_details_ui(
    ui: &mut egui::Ui,
    modbus_protocol_definitions: &mut ModbusDefinitions,
) -> egui::InnerResponse<()> {
    ui.horizontal(|ui| {
        let mut modbus_request_vec = Vec::new();
        ui.label("Request:");
        let mut modbus_request = ModbusRequest::new(1, ModbusProto::TcpUdp);
        match modbus_protocol_definitions.register_type {
            RegisterType::Holding => {
                if modbus_request
                    .generate_get_holdings(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
            RegisterType::Inputs => {
                if modbus_request
                    .generate_get_inputs(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
            RegisterType::Coils => {
                if modbus_request
                    .generate_get_coils(
                        modbus_protocol_definitions.start_address,
                        modbus_protocol_definitions.register_count,
                        &mut modbus_request_vec,
                    )
                    .is_ok()
                {}
            }
        }
        for i in 0..modbus_request_vec.len() {
            ui.label(format!("{:02X}", modbus_request_vec[i]));
        }
        modbus_protocol_definitions.request_function_vec = modbus_request_vec;
    })
}

fn modbus_serial_device_ui(device_config_buffer: &mut DeviceConfigUiBuffer, ui: &mut egui::Ui) {
    ComboBox::from_label(format!("{} Port", egui_phosphor::regular::USB))
        .selected_text(device_config_buffer.modbus_serial_buffer.port.clone())
        .show_ui(ui, |ui| {
            if let Ok(mut ports) = available_ports() {
                for port in ports.iter_mut() {
                    ui.selectable_value(
                        &mut device_config_buffer.modbus_serial_buffer.port,
                        port.clone().port_name,
                        format!("{}", port.port_name),
                    );
                }
            }
        });
    ComboBox::from_label("Baudrate")
        .selected_text(format!(
            "{}",
            device_config_buffer.modbus_serial_buffer.baudrate.clone()
        ))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut device_config_buffer.modbus_serial_buffer.baudrate,
                Baudrate::Baud38400,
                "38400",
            );
            ui.selectable_value(
                &mut device_config_buffer.modbus_serial_buffer.baudrate,
                Baudrate::Baud9600,
                "9600",
            );
        });
    ComboBox::from_label("Parity")
        .selected_text(format!(
            "{}",
            device_config_buffer.modbus_serial_buffer.parity.clone()
        ))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut device_config_buffer.modbus_serial_buffer.parity,
                Parity::Even,
                "Even",
            );
            ui.selectable_value(
                &mut device_config_buffer.modbus_serial_buffer.parity,
                Parity::Odd,
                "Odd",
            );
            ui.selectable_value(
                &mut device_config_buffer.modbus_serial_buffer.parity,
                Parity::NoneParity,
                "None",
            );
        });

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut device_config_buffer.modbus_serial_buffer.slave_buffer)
                .desired_width(50.),
        );
        ui.label("Slave");
    });
    if let Ok(slave) = device_config_buffer
        .modbus_serial_buffer
        .slave_buffer
        .parse::<u8>()
    {
        device_config_buffer.modbus_serial_buffer.slave = slave;
    } else {
        ui.colored_label(Color32::DARK_RED, "Non valid slave address.");
    }
}

fn modbus_protocol_ui(modbus_protocol_definitions: &mut ModbusDefinitions, ui: &mut egui::Ui) {
    ComboBox::from_label("Register Type")
        .selected_text(format!("{}", modbus_protocol_definitions.register_type))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut modbus_protocol_definitions.register_type,
                RegisterType::Coils,
                "Coils",
            );
            ui.selectable_value(
                &mut modbus_protocol_definitions.register_type,
                RegisterType::Inputs,
                "Input registers",
            );
            ui.selectable_value(
                &mut modbus_protocol_definitions.register_type,
                RegisterType::Holding,
                "Holding registers",
            );
        });

    ui.add(
        Slider::new(&mut modbus_protocol_definitions.start_address, 0..=9999).text("Start Address"),
    );
    ui.add(Slider::new(&mut modbus_protocol_definitions.register_count, 1..=1000).text("Quantity"));
    ui.add(
        Slider::new(&mut modbus_protocol_definitions.scan_delay, 200..=10000)
            .text("Scan Delay (ms)"),
    );
}

fn modbus_tcp_device_ui(ui: &mut egui::Ui, device_config_buffer: &mut DeviceConfigUiBuffer) {
    ui.label("IP Address");
    ui.add(
        egui::TextEdit::singleline(&mut device_config_buffer.modbus_tcp_buffer.ip_address)
            .desired_width(120.),
    );
    ui.add(Slider::new(&mut device_config_buffer.modbus_tcp_buffer.port, 0..=10000).text("Port"));
}

fn s7_device_ui(ui: &mut egui::Ui, device_config_buffer: &mut DeviceConfigUiBuffer) {
    ui.label("PLC IP Address");
    ui.add(egui::TextEdit::singleline(&mut device_config_buffer.s7_buffer.ip).desired_width(120.));
}
fn spawn_polling_thread(
    device_config: &mut DeviceConfig,
    mutex: Arc<Mutex<MutexData>>,
    logger_path: &PathBuf,
) {
    let paths = std::fs::read_dir(".").unwrap();

    let matches = paths
        .map(|path| path.unwrap())
        .filter(|path| path.metadata().unwrap().len() < 100_000)
        .filter(|path| path.file_name().is_ascii().to_string().starts_with("LOG"));
    let mut logger = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(logger_path)
        .unwrap();

    match device_config {
        DeviceConfig::ModbusSerial(config) => {
            let baudrate_match = match config.baudrate {
                Baudrate::Baud38400 => 38400,
                Baudrate::Baud9600 => 9600,
            };
            let parity = match config.parity {
                Parity::Even => serialport::Parity::Even,
                Parity::Odd => serialport::Parity::Odd,
                Parity::NoneParity => serialport::Parity::None,
            };

            //spawn_serial_polling_thread(, , , , , )
            let mut config = config.clone();
            thread::spawn(move || {
                let serial = serialport::new(config.port, baudrate_match)
                    .parity(parity)
                    .timeout(Duration::from_millis(1500));
                let ctx = connect_slave(&serial, Slave(config.slave));
                if let Ok(mut ctx) = ctx {
                    loop {
                        thread::sleep(Duration::from_millis(
                            config.protocol_definitions.scan_delay,
                        ));
                        if let Some(mut mutex) = mutex.try_lock() {
                            // We check for any pending new modbus configuration
                            if let Some(new_config) = mutex.new_config.clone() {
                                // We update the modbus config
                                config.protocol_definitions =
                                    new_config.modbus_serial_buffer.protocol_definitions;

                                // We clean the mutex
                                mutex.new_config = None;
                            }

                            // We check for a pending thread kill request
                            if mutex.kill_thread {
                                // We clean the mutex
                                mutex.kill_thread = false;

                                // We return from the thread
                                return;
                            }
                        }

                        match config.protocol_definitions.register_type {
                            RegisterType::Coils => {}
                            RegisterType::Inputs => {
                                let now = Instant::now();
                                let result = ctx.read_input_registers(
                                    config.protocol_definitions.start_address,
                                    config.protocol_definitions.register_count,
                                );
                                if let Ok(res) = result {
                                    let elapsed_time = now.elapsed().as_micros();
                                    let mut data = mutex.lock();
                                    data.data = res;
                                    data.achieved_scan_time = elapsed_time;
                                }
                            }
                            RegisterType::Holding => {
                                let now = Instant::now();
                                let result = ctx.read_holding_registers(
                                    config.protocol_definitions.start_address,
                                    config.protocol_definitions.register_count,
                                );
                                if let Ok(res) = result {
                                    let elapsed_time = now.elapsed().as_micros();
                                    let mut log = true;
                                    {
                                        let mut data = mutex.lock();
                                        data.data = res.clone();
                                        data.achieved_scan_time = elapsed_time;
                                        log = data.log;
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
        DeviceConfig::S7(s7_config) => {
            let s7_config = s7_config.clone();

            if let Ok(addr) = s7_config.ip.parse::<Ipv4Addr>() {
                thread::spawn(move || {
                    //let addr = Ipv4Addr::new(127, 0, 0, 1);
                    let mut opts = tcp::Options::new(IpAddr::from(addr), 5, 5, Connection::PG);
                    opts.read_timeout = Duration::from_secs(5);
                    opts.write_timeout = Duration::from_secs(5);

                    if let Ok(t) = tcp::Transport::connect(opts) {
                        let mut cl = Client::new(t).unwrap();
                        let offset1 = 4.0;
                        let offset2 = 8.0;
                        let db = DB;

                        loop {
                            thread::sleep(Duration::from_millis(1000));

                            let mut s7_message = None;
                            let mut buffer1 = vec![0u8; Float::size() as usize];
                            let mut buffer2 = vec![0u8; Float::size() as usize];
                            let now = Instant::now();
                            cl.ag_read(db, offset1 as i32, Float::size(), buffer1.as_mut())
                                .unwrap();
                            cl.ag_read(db, offset2 as i32, Float::size(), buffer2.as_mut())
                                .unwrap();

                            let elapsed_time = now.elapsed().as_micros();
                            let tag1 = Float::new(db, offset1, buffer1).unwrap();
                            let value1: f32 = tag1.value();

                            let tag2 = Float::new(db, offset2, buffer2).unwrap();
                            let value2: f32 = tag2.value();

                            {
                                let mut data = mutex.lock();
                                data.s7_read_data.tag1 = value1;
                                data.s7_read_data.tag2 = value2;
                                s7_message = data.s7_message.clone();
                                data.achieved_scan_time = elapsed_time;
                                data.s7_message = None;
                            }

                            if let Some(msg) = s7_message {
                                match msg.message {
                                    S7Message::S7Bool(value) => {
                                        let mut buffer = vec![0u8; Bool::size() as usize];
                                        if cl
                                            .ag_read(
                                                msg.db,
                                                msg.offset as i32,
                                                Bool::size(),
                                                &mut buffer,
                                            )
                                            .is_ok()
                                        {
                                            let mut v =
                                                Bool::new(msg.db, msg.offset, buffer.to_vec())
                                                    .unwrap();
                                            v.set_value(!v.value());

                                            let fields: Fields = vec![Box::new(v)];
                                            for field in fields.iter() {
                                                cl.ag_write(
                                                    field.data_block(),
                                                    field.offset(),
                                                    field.to_bytes().len() as i32,
                                                    field.to_bytes().as_mut(),
                                                )
                                                .unwrap();
                                            }
                                        }
                                    }
                                    S7Message::S7Real(value) => {}
                                    S7Message::S7Int(value) => {}
                                }
                                s7_message = None;
                            }
                        }
                    } else {
                        println!("Could not connect tcp.");
                    }
                });
            }
        }
        DeviceConfig::ModbusTcp(config) => {
            let mut config = config.clone();
            let tcp_string = format!("{}:{}", config.ip_address, config.port);
            thread::spawn(move || {
                if let Ok(sock_addr) = tcp_string.parse::<SocketAddr>() {
                    let ctx = connect_with_timeout(sock_addr, Some(Duration::from_millis(5000)));
                    if let Ok(mut ctx) = ctx {
                        loop {
                            thread::sleep(Duration::from_millis(
                                config.protocol_definitions.scan_delay,
                            ));
                            if let Some(mut mutex) = mutex.try_lock() {
                                // We check for any pending new modbus configuration
                                if let Some(new_config) = mutex.new_config.clone() {
                                    // We update the modbus config
                                    config.protocol_definitions =
                                        new_config.modbus_tcp_buffer.protocol_definitions;
                                    // We clean the mutex
                                    mutex.new_config = None;
                                }
                                // We check for a pending thread kill request
                                if mutex.kill_thread {
                                    // We clean the mutex
                                    mutex.kill_thread = false;
                                    // We return from the thread
                                    return;
                                }

                                if let Some(modbus_msg) = mutex.modbus_float_message {
                                    let data = float_to_u16(modbus_msg.value);
                                    let res = ctx.write_multiple_registers(
                                        modbus_msg.register,
                                        &[data.0, data.1],
                                    );
                                    if res.is_ok() {
                                        mutex.modbus_float_message = None;
                                    }
                                }
                                if let Some(modbus_msg) = mutex.modbus_bool_message {
                                    let res = ctx
                                        .write_single_coil(modbus_msg.register, modbus_msg.value);
                                    if res.is_ok() {
                                        mutex.modbus_bool_message = None;
                                    }
                                }
                            }

                            match config.protocol_definitions.register_type {
                                RegisterType::Coils => {}
                                RegisterType::Inputs => {
                                    let now = Instant::now();
                                    let result = ctx.read_input_registers(
                                        config.protocol_definitions.start_address,
                                        config.protocol_definitions.register_count,
                                    );

                                    if let Ok(res) = result {
                                        let elapsed_time = now.elapsed().as_micros();
                                        let mut data = mutex.lock();
                                        data.data = res;
                                        data.achieved_scan_time = elapsed_time;
                                    }
                                }
                                RegisterType::Holding => {
                                    let now = Instant::now();
                                    let result = ctx.read_holding_registers(
                                        config.protocol_definitions.start_address,
                                        config.protocol_definitions.register_count,
                                    );
                                    match result {
                                        Ok(res) => {
                                            let elapsed_time = now.elapsed().as_micros();
                                            {
                                                let mut data = mutex.lock();
                                                data.data = res.clone();
                                                data.error_msg = "".to_string();
                                                data.achieved_scan_time = elapsed_time;
                                            }
                                            let tag_list = [
                                                "FLP", "FLT", "FLR", "HPP", "CHT1", "CHT2", "CHT3",
                                                "CHT4", "CHS1", "CHS2", "CHS3", "CHS4", "BPP",
                                                "DTL", "FUDP", "WTL",
                                            ];
                                            if true {
                                                if res.len() >= (tag_list.len() * 2) {
                                                    let mut line = String::new();
                                                    let datetime = chrono::Local::now();
                                                    let datetime =
                                                        datetime.format("%d/%m/%Y\t %H:%M:%S\t");
                                                    line.push_str(&datetime.to_string());
                                                    let mut i = 0;
                                                    for _tag in tag_list.iter() {
                                                        let fmt = format!(
                                                            "{:.2}\t",
                                                            u16_to_float(
                                                                res[i * 2],
                                                                res[(i * 2) + 1]
                                                            )
                                                        );
                                                        line.push_str(&format!("{}", &fmt));
                                                        i += 1;
                                                    }

                                                    line.push_str("\r\n");
                                                    logger.write_all(line.as_bytes()).unwrap();
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let error_code = 2;
                                            let error_msg = format!(
                                                "{:#02x}: Could not read registers. {}",
                                                error_code, e
                                            );
                                            let mut data = mutex.lock();
                                            data.error_msg = error_msg;
                                            data.achieved_scan_time = 0;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let error_code = 1;
                        let error_msg =
                            format!("{:#02x}: Could not connect to server.", error_code);
                        let mut data = mutex.lock();
                        data.error_msg = error_msg;
                        data.achieved_scan_time = 0;
                        return;
                    }
                } else {
                    let error_code = 3;
                    let error_msg = format!("{:#02x}: Error parsing the address IP.", error_code);
                    let mut data = mutex.lock();
                    data.error_msg = error_msg;
                    data.achieved_scan_time = 0;
                    return;
                }
            });
        }
        _ => {}
    }
}

async fn index(req: HttpRequest) -> &'static str {
    log::info!("REQ: {req:?}");
    "Hello world!"
}

async fn run_app() -> std::io::Result<()> {
    log::info!("starting HTTP server at http://localhost:8080");

    // srv is server controller type, `dev::Server`
    let server = HttpServer::new(|| {
        App::new()
            // enable logger
            .wrap(middleware::Logger::default())
            .service(web::resource("/index.html").to(|| async { "Hello world!" }))
            .service(web::resource("/").to(index))
    })
    .bind(("127.0.0.1", 8080))?
    .workers(2)
    .run();

    // Send server handle back to the main thread
    //let _ = tx.send(server.handle());

    server.await
}

fn u16_to_float(reg1: u16, reg2: u16) -> f32 {
    let data_32bit_rep = ((reg1 as u32) << 16) | reg2 as u32;
    let data_array = data_32bit_rep.to_ne_bytes();
    f32::from_ne_bytes(data_array)
}

fn float_to_u16(value: f32) -> (u16, u16) {
    let value = value.to_ne_bytes();
    let value = u32::from_ne_bytes(value);
    let high = value & 0x0000FFFF;
    let low = (value & 0xFFFF0000) >> 16;

    let low = low as u16;
    let high = high as u16;

    (low, high)
}
