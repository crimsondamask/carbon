use egui::{
    style::Selection, Button, Color32, ComboBox, Label, RichText, Rounding, Slider, Stroke, Vec2,
    Visuals,
};
use egui_phosphor;
use epaint::Pos2;
use image::math::Rect;
use parking_lot::Mutex;
use rmodbus::{client::ModbusRequest, ModbusProto};
//use rseip::precludes::*;
use serialport::available_ports;
use std::{
    fmt::Display,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tokio_modbus::prelude::{sync::rtu::connect_slave, sync::tcp::connect, *};

use actix_web::{middleware, rt, web, App, HttpRequest, HttpServer};

use s7::{client::Client, field::Bool, field::Fields, field::Float, tcp, transport::Connection};
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
}
//####################################################

//#################################################### The Mutex used between
//the main and background threads.
struct MutexData {
    data: Vec<u16>,
    s7_read_data: S7Data,
    s7_message: Option<S7MessageTag>,
    achieved_scan_time: u128,
    new_config: Option<DeviceConfigUiBuffer>,
    kill_thread: bool,
}
//####################################################

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
            register_count: 1,
            scan_delay: 500,
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
                achieved_scan_time: 0,
                new_config: None,
                kill_thread: false,
            })),
            protocol: Protocol::default(),
            device_config: DeviceConfig::default(),
            tag1: 0.0,
            tag2: 0.0,
            tag3: 0.0,
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

        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(197, 197, 197);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(197, 197, 197);
        visuals.widgets.inactive.rounding = Rounding::ZERO;
        visuals.widgets.noninteractive.rounding = Rounding::ZERO;
        visuals.widgets.active.rounding = Rounding::ZERO;
        visuals.widgets.hovered.rounding = Rounding::ZERO;
        visuals.window_rounding = Rounding::ZERO;
        visuals.menu_rounding = Rounding::ZERO;
        visuals.panel_fill = Color32::from_rgb(221, 221, 221);
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
        } = self;

        ctx.request_repaint();
        #[cfg(not(target_arch = "wasm32"))] // no File->Quit on web pages!
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        _frame.close();
                    }
                });
            });
        });
        egui::TopBottomPanel::bottom("bottom-panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 30.0;
                    ui.colored_label(Color32::GRAY, "Carbon v0.1");

                    if let Some(data) = mutex.try_lock() {
                        let achieved_scan_time = data.achieved_scan_time;
                        ui.colored_label(
                            Color32::GRAY,
                            format!("Achieved scan time: {} μs", achieved_scan_time),
                        );
                    }
                });
            });
        });

        egui::SidePanel::right("side_panel")
            .exact_width(220.)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{} Protocol Configuration",
                    egui_phosphor::regular::GEAR_SIX
                ));

                ui.horizontal(|ui| {
                    ComboBox::from_label("Protocol")
                        .selected_text(format!("{}", protocol))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                protocol,
                                Protocol::ModbusTcpProtocol,
                                "Modbus TCP",
                            );
                            ui.selectable_value(
                                protocol,
                                Protocol::ModbusRtuProtocol,
                                "Modbus Serial",
                            );
                            ui.selectable_value(
                                protocol,
                                Protocol::EthernetIpProtocol,
                                "EthernetIP",
                            );
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

                            // Spawn the data polling thread.
                            spawn_polling_thread(device_config, mutex);

                            thread::spawn(move || {
                                let server_future = run_app();
                                rt::System::new().block_on(server_future)
                            });
                        }
                        if !app_run_state.is_ui_apply_clicked {
                            // if ui
                            //     .add_enabled(
                            //         app_run_state.enable_proto_opt_edit
                            //             && app_run_state.is_loop_running,
                            //         Button::new(format!("Edit")).min_size(Vec2::new(100., 10.)),
                            //     )
                            //     //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                            //     .clicked()
                            // {
                            //     app_run_state.is_ui_apply_clicked = true;
                            // }

                            // if ui
                            //     .add_enabled(
                            //         app_run_state.is_loop_running,
                            //         Button::new(format!("Disconnect"))
                            //             .min_size(Vec2::new(100., 10.)),
                            //     )
                            //     .clicked()
                            // {
                            //     let mutex = Arc::clone(&mutex);
                            //     mutex.lock().kill_thread = true;
                            //     app_run_state.is_loop_running = false;
                            //     app_run_state.is_ui_apply_clicked = false;
                            //     app_run_state.enable_device_opt_edit = true;
                            // }
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(data) = mutex.try_lock() {
                let achieved_scan_time = data.achieved_scan_time;
                ui.label(format!("Achieved scan time: {} μs", achieved_scan_time));
            }
            /*
            if ui.button("Play").clicked() {
                thread::spawn(move || {
                    let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
                    let alarm = std::fs::File::open("alarm.wav").unwrap();
                    let beep1 = stream_handle.play_once(BufReader::new(alarm)).unwrap();
                    beep1.set_volume(0.5);
                    thread::sleep(Duration::from_millis(1500))
                });
            }
            */
            ui.separator();
            /*
            egui::Grid::new("Data Table")
                .num_columns(4)
                .min_col_width(200.)
                .striped(true)
                .min_row_height(20.)
                .show(ui, |ui| {
                    ui.label("Address");
                    ui.label("Value (Decimal)");
                    ui.label("Value (Hex)");
                    ui.end_row();
                    if let Some(data) = mutex.try_lock() {
                        for (i, val) in data.data.iter().enumerate() {
                            ui.label(format!("{}", i));
                            ui.label(format!("{:.2}", val));
                            ui.label(format!("{:#06X}", val));
                            ui.end_row();
                        }
                    }
                });
            */
            {
                if let Some(data) = mutex.try_lock() {
                    *tag1 = data.s7_read_data.tag1;
                    *tag2 = data.s7_read_data.tag2;
                    *tag3 = data.s7_read_data.tag3;
                }
            }
            egui::Image::new(egui::include_image!("../assets/sample.png"))
                .paint_at(ui, ui.ctx().available_rect());
            if ui
                .put(
                    egui::Rect {
                        min: Pos2::new(50., 50.),

                        max: Pos2::new(150., 100.),
                    },
                    Button::new("Hello"),
                )
                .clicked()
            {}

            egui::Grid::new("HMI")
                .num_columns(3)
                .min_col_width(100.)
                .min_row_height(100.)
                .show(ui, |ui| {
                    if ui
                        .add(
                            Button::new("Button")
                                .min_size(Vec2 { x: 80., y: 80. })
                                .fill(Color32::GREEN),
                        )
                        .clicked()
                    {}
                    if ui
                        .add(
                            Button::new("Button")
                                .min_size(Vec2 { x: 80., y: 80. })
                                .fill(Color32::RED),
                        )
                        .clicked()
                    {}
                    if ui
                        .add(Button::new("Button").min_size(Vec2 { x: 80., y: 80. }))
                        .clicked()
                    {
                        if let Some(mut data) = mutex.try_lock() {
                            data.s7_message = Some(S7MessageTag {
                                message: S7Message::S7Bool(true),
                                db: 1,
                                offset: 2.1,
                            });
                        }
                    }
                    ui.end_row();
                    ui.add(Label::new(
                        RichText::new(format!("  {:.02} barg  ", tag1))
                            .size(18.)
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(Color32::BLACK),
                    ));
                    ui.add(Label::new(
                        RichText::new(format!("  {:.02} barg  ", tag2))
                            .size(18.)
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(Color32::BLACK),
                    ));
                    ui.add(Label::new(
                        RichText::new(format!("  {:.02} barg  ", tag3))
                            .size(18.)
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(Color32::BLACK),
                    ));
                });
        });
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
fn spawn_polling_thread(device_config: &mut DeviceConfig, mutex: Arc<Mutex<MutexData>>) {
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
                                    let mut data = mutex.lock();
                                    data.data = res;
                                    data.achieved_scan_time = elapsed_time;
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
                            cl.ag_read(db, offset1 as i32, Float::size(), buffer1.as_mut())
                                .unwrap();
                            cl.ag_read(db, offset2 as i32, Float::size(), buffer2.as_mut())
                                .unwrap();

                            let mut tag1 = Float::new(db, offset1, buffer1).unwrap();
                            let value1: f32 = tag1.value();

                            let mut tag2 = Float::new(db, offset2, buffer2).unwrap();
                            let value2: f32 = tag2.value();

                            {
                                let mut data = mutex.lock();
                                data.s7_read_data.tag1 = value1;
                                data.s7_read_data.tag2 = value2;
                                s7_message = data.s7_message.clone();
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
            let ctx = connect(tcp_string.parse().unwrap());
            thread::spawn(move || {
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
                                    let mut data = mutex.lock();
                                    data.data = res;
                                    data.achieved_scan_time = elapsed_time;
                                }
                            }
                        }
                    }
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
