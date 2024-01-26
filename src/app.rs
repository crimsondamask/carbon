use egui::{style::Selection, Button, Color32, ComboBox, Rounding, Slider, Stroke, Vec2, Visuals};
use egui_phosphor;
use parking_lot::Mutex;
use rmodbus::{client::ModbusRequest, ModbusProto};
//use rseip::precludes::*;
use serialport::available_ports;
use std::{fmt::Display, sync::Arc, thread, time::Duration};
use tokio_modbus::prelude::{sync::rtu::connect_slave, *};

//#################################################### Main App Struct

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct CarbonApp {
    device_config_buffer: DeviceConfigUiBuffer,
    run_state: i32,
    #[serde(skip)]
    is_running: bool,
    #[serde(skip)]
    is_apply_clicked: bool,
    #[serde(skip)]
    enable_register_edit: bool,
    #[serde(skip)]
    enable_device_edit: bool,
    #[serde(skip)]
    mutex: Arc<Mutex<MutexData>>,
    protocol: Protocol,
    #[serde(skip)]
    device_config: DeviceConfig,
    protocol_definitions: ModbusDefinitions,
}
//####################################################

//#################################################### The Mutex used between
//the main and background threads.
struct MutexData {
    data: Vec<u16>,
    new_device_config: Option<DeviceConfig>,
    new_modbus_config: Option<ModbusDefinitions>,
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
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::ModbusTcpProtocol => write!(f, "Modbus TCP"),
            Protocol::ModbusRtuProtocol => write!(f, "Modbus Serial"),
            Protocol::EthernetIpProtocol => write!(f, "Ethernet/IP"),
            Protocol::S7Protocol => write!(f, "Siemens S7"),
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
            s7_buffer: S7Config,
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
struct S7Config;

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

/*

#[derive(serde::Deserialize, serde::Serialize, Clone)]
enum ProtocolDefinitions {
    ModbusProtocolDefinitions(ModbusDefinitions),
    EthernetIpProtocolDefinitions,
    S7ProtocolDefinitions,
}

impl Default for ProtocolDefinitions {
    fn default() -> Self {
        Self::ModbusProtocolDefinitions(ModbusDefinitions::default())
    }
    }
*/
// ###################################################

impl Default for CarbonApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            device_config_buffer: DeviceConfigUiBuffer::default(),
            run_state: 0,
            is_running: false,
            enable_register_edit: true,
            is_apply_clicked: false,
            enable_device_edit: true,
            mutex: Arc::new(Mutex::new(MutexData {
                data: Vec::new(),
                new_device_config: None,
                new_modbus_config: None,
                kill_thread: false,
            })),
            protocol: Protocol::default(),
            device_config: DeviceConfig::default(),
            protocol_definitions: ModbusDefinitions::default(),
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
            device_config_buffer,
            run_state,
            is_running,
            enable_register_edit,
            is_apply_clicked,
            enable_device_edit,
            mutex,
            protocol,
            device_config,
            protocol_definitions,
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
                    //ui.spacing_mut().item_spacing.x = 0.0;
                    ui.colored_label(Color32::GRAY, "Carbon v0.1");
                });
            });
        });

        egui::SidePanel::right("side_panel")
            .exact_width(380.)
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
                    }
                });

                match protocol {
                    Protocol::EthernetIpProtocol => {}
                    Protocol::S7Protocol => {}
                    Protocol::ModbusTcpProtocol => {
                        // Modbus TCP UI
                        // Switch to ModbusTcpConfig

                        ui.group(|ui| {
                            ui.set_enabled(*enable_device_edit);
                            ui.label(format!("{} Device Options", egui_phosphor::regular::WRENCH));
                            ui.vertical(|ui| {
                                ui.label("IP Address");
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut device_config_buffer.modbus_tcp_buffer.ip_address,
                                    )
                                    .desired_width(120.),
                                );
                                ui.add(
                                    Slider::new(
                                        &mut device_config_buffer.modbus_tcp_buffer.port,
                                        0..=10000,
                                    )
                                    .text("Port"),
                                );
                            });
                        });

                        ui.separator();
                        ui.group(|ui| {
                            ui.set_enabled(*is_apply_clicked || !*is_running);
                            ui.label(format!(
                                "{} Request Options",
                                egui_phosphor::regular::WRENCH
                            ));

                            ComboBox::from_label("Register Type")
                                .selected_text(format!(
                                    "{}",
                                    device_config_buffer
                                        .modbus_tcp_buffer
                                        .protocol_definitions
                                        .register_type
                                ))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut device_config_buffer
                                            .modbus_tcp_buffer
                                            .protocol_definitions
                                            .register_type,
                                        RegisterType::Coils,
                                        "Coils",
                                    );
                                    ui.selectable_value(
                                        &mut device_config_buffer
                                            .modbus_tcp_buffer
                                            .protocol_definitions
                                            .register_type,
                                        RegisterType::Inputs,
                                        "Input registers",
                                    );
                                    ui.selectable_value(
                                        &mut device_config_buffer
                                            .modbus_tcp_buffer
                                            .protocol_definitions
                                            .register_type,
                                        RegisterType::Holding,
                                        "Holding registers",
                                    );
                                });

                            ui.add(
                                Slider::new(
                                    &mut device_config_buffer
                                        .modbus_tcp_buffer
                                        .protocol_definitions
                                        .start_address,
                                    1..=9999,
                                )
                                .text("Start Address"),
                            );
                            ui.add(
                                Slider::new(
                                    &mut device_config_buffer
                                        .modbus_tcp_buffer
                                        .protocol_definitions
                                        .register_count,
                                    1..=1000,
                                )
                                .text("Quantity"),
                            );
                            ui.add(
                                Slider::new(
                                    &mut device_config_buffer
                                        .modbus_tcp_buffer
                                        .protocol_definitions
                                        .scan_delay,
                                    1..=10000,
                                )
                                .text("Scan Delay (ms)"),
                            );
                        });

                        ui.group(|ui| {
                            ui.set_enabled(false);
                            ui.horizontal(|ui| {
                                let mut modbus_request_vec = Vec::new();
                                ui.label("Request:");
                                let mut modbus_request = ModbusRequest::new(1, ModbusProto::TcpUdp);
                                match device_config_buffer
                                    .modbus_tcp_buffer
                                    .protocol_definitions
                                    .register_type
                                {
                                    RegisterType::Holding => {
                                        if modbus_request
                                            .generate_get_holdings(
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .start_address,
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .register_count,
                                                &mut modbus_request_vec,
                                            )
                                            .is_ok()
                                        {}
                                    }
                                    RegisterType::Inputs => {
                                        if modbus_request
                                            .generate_get_inputs(
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .start_address,
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .register_count,
                                                &mut modbus_request_vec,
                                            )
                                            .is_ok()
                                        {}
                                    }
                                    RegisterType::Coils => {
                                        if modbus_request
                                            .generate_get_coils(
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .start_address,
                                                device_config_buffer
                                                    .modbus_tcp_buffer
                                                    .protocol_definitions
                                                    .register_count,
                                                &mut modbus_request_vec,
                                            )
                                            .is_ok()
                                        {}
                                    }
                                }
                                for i in 0..modbus_request_vec.len() {
                                    ui.label(format!("{:02X}", modbus_request_vec[i]));
                                }
                                device_config_buffer
                                    .modbus_tcp_buffer
                                    .protocol_definitions
                                    .request_function_vec = modbus_request_vec;
                            });
                        });

                        *device_config =
                            DeviceConfig::ModbusTcp(device_config_buffer.modbus_tcp_buffer.clone());
                    }
                    Protocol::ModbusRtuProtocol => {
                        // Modbus serial UI
                        // Switch to ModbusSerialConfig

                        ui.group(|ui| {
                            ui.set_enabled(*enable_device_edit);
                            ui.label(format!("{} Device Options", egui_phosphor::regular::WRENCH));
                            match device_config {
                                DeviceConfig::ModbusSerial(config) => {
                                    {
                                        let config: &mut ModbusSerialConfig = config;
                                        ComboBox::from_label(format!(
                                            "{} Port",
                                            egui_phosphor::regular::USB
                                        ))
                                        .selected_text(config.port.clone())
                                        .show_ui(
                                            ui,
                                            |ui| {
                                                if let Ok(mut ports) = available_ports() {
                                                    for port in ports.iter_mut() {
                                                        ui.selectable_value(
                                                            &mut config.port,
                                                            port.clone().port_name,
                                                            format!("{}", port.port_name),
                                                        );
                                                    }
                                                }
                                            },
                                        );
                                        ComboBox::from_label("Baudrate")
                                            .selected_text(format!("{}", config.baudrate.clone()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut config.baudrate,
                                                    Baudrate::Baud38400,
                                                    "38400",
                                                );
                                                ui.selectable_value(
                                                    &mut config.baudrate,
                                                    Baudrate::Baud9600,
                                                    "9600",
                                                );
                                            });
                                        ComboBox::from_label("Parity")
                                            .selected_text(format!("{}", config.parity.clone()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut config.parity,
                                                    Parity::Even,
                                                    "Even",
                                                );
                                                ui.selectable_value(
                                                    &mut config.parity,
                                                    Parity::Odd,
                                                    "Odd",
                                                );
                                                ui.selectable_value(
                                                    &mut config.parity,
                                                    Parity::NoneParity,
                                                    "None",
                                                );
                                            });

                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut config.slave_buffer,
                                                )
                                                .desired_width(50.),
                                            );
                                            ui.label("Slave");
                                        });
                                        if let Ok(slave) = config.slave_buffer.parse::<u8>() {
                                            config.slave = slave;
                                        } else {
                                            ui.colored_label(
                                                Color32::DARK_RED,
                                                "Non valid slave address.",
                                            );
                                        }
                                    };
                                }
                                _ => {}
                            };
                        });
                    }
                }

                ui.separator();
                egui::Grid::new("Buttons")
                    .num_columns(3)
                    .min_col_width(100.)
                    .show(ui, |ui| {
                        if ui
                            .add_enabled(
                                !*is_running,
                                Button::new(format!("Connect")).min_size(Vec2::new(100., 10.)),
                            )
                            //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                            .clicked()
                        {
                            *enable_device_edit = false;
                            *enable_register_edit = true;
                            *is_running = true;
                            let mutex = Arc::clone(&mutex);
                            spawn_polling_thread(device_config, mutex);
                        }
                        if !*is_apply_clicked {
                            if ui
                                .add_enabled(
                                    *enable_register_edit && *is_running,
                                    Button::new(format!("Edit")).min_size(Vec2::new(100., 10.)),
                                )
                                //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                                .clicked()
                            {
                                *is_apply_clicked = true;
                            }

                            if ui
                                .add_enabled(
                                    *is_running,
                                    Button::new(format!("Disconnect"))
                                        .min_size(Vec2::new(100., 10.)),
                                )
                                .clicked()
                            {
                                let mutex = Arc::clone(&mutex);
                                mutex.lock().kill_thread = true;
                                *is_running = false;
                                *is_apply_clicked = false;
                                *enable_device_edit = true;
                            }
                        } else {
                            if ui
                                .add_enabled(
                                    *enable_register_edit && *is_running,
                                    Button::new(format!("{} Apply", egui_phosphor::regular::PEN)),
                                )
                                //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                                .clicked()
                            {
                                let mutex = Arc::clone(&mutex);
                                mutex.lock().new_modbus_config = Some(protocol_definitions.clone());
                                *is_apply_clicked = false;
                            }
                        }
                    });
                // let button = Button::new(format!("{} Connect", egui_phosphor::regular::PLUGS))
                //ui.add(egui::Image::new(egui::include_image!("../assets/pdu.png")));
            });

        egui::CentralPanel::default()
            //.frame(Frame {
            //fill: Color32::from_rgb(190, 190, 190),
            //..Default::default()
            //})
            .show(ctx, |ui| {
                // The central panel the region left after adding TopPanel's and SidePanel's
                egui::Grid::new("Data Table")
                    .num_columns(4)
                    .min_col_width(200.)
                    .striped(true)
                    .min_row_height(20.)
                    .show(ui, |ui| {
                        ui.label("Register");
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
                //egui::Window::new("Modbus Request Details").open(&mut true).show(ctx, |ui| {});
            });
    }
}

fn modbus_serial_ui(config: &mut ModbusSerialConfig, ui: &mut egui::Ui) {
    ComboBox::from_label(format!("{} Port", egui_phosphor::regular::USB))
        .selected_text(config.port.clone())
        .show_ui(ui, |ui| {
            if let Ok(mut ports) = available_ports() {
                for port in ports.iter_mut() {
                    ui.selectable_value(
                        &mut config.port,
                        port.clone().port_name,
                        format!("{}", port.port_name),
                    );
                }
            }
        });
    ComboBox::from_label("Baudrate")
        .selected_text(format!("{}", config.baudrate.clone()))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut config.baudrate, Baudrate::Baud38400, "38400");
            ui.selectable_value(&mut config.baudrate, Baudrate::Baud9600, "9600");
        });
    ComboBox::from_label("Parity")
        .selected_text(format!("{}", config.parity.clone()))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut config.parity, Parity::Even, "Even");
            ui.selectable_value(&mut config.parity, Parity::Odd, "Odd");
            ui.selectable_value(&mut config.parity, Parity::NoneParity, "None");
        });

    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut config.slave_buffer).desired_width(50.));
        ui.label("Slave");
    });
    if let Ok(slave) = config.slave_buffer.parse::<u8>() {
        config.slave = slave;
    } else {
        ui.colored_label(Color32::DARK_RED, "Non valid slave address.");
    }
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
                let ctx = connect_slave(&serial, Slave(1));
                if let Ok(mut ctx) = ctx {
                    loop {
                        thread::sleep(Duration::from_millis(
                            config.protocol_definitions.scan_delay,
                        ));
                        if let Some(mut mutex) = mutex.try_lock() {
                            // We check for any pending new modbus configuration
                            if let Some(new_modbus_config) = mutex.new_modbus_config.clone() {
                                // We update the modbus config
                                config.protocol_definitions = new_modbus_config;

                                // We clean the mutex
                                mutex.new_device_config = None;
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
                                let result = ctx.read_input_registers(
                                    config.protocol_definitions.start_address,
                                    config.protocol_definitions.register_count,
                                );
                                if let Ok(res) = result {
                                    let mut data = mutex.lock();
                                    data.data = res;
                                }
                            }
                            RegisterType::Holding => {
                                let result = ctx.read_holding_registers(
                                    config.protocol_definitions.start_address,
                                    config.protocol_definitions.register_count,
                                );
                                if let Ok(res) = result {
                                    let mut data = mutex.lock();
                                    data.data = res;
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
