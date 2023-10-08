use egui::{Button, Color32, ComboBox, FontDefinitions, Slider};
use egui_phosphor;
use parking_lot::Mutex;
use serialport::{available_ports, SerialPort, SerialPortBuilder};
use std::{fmt::Display, sync::Arc, thread, time::Duration};
use tokio_modbus::prelude::{sync::rtu::connect_slave, *};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    address_buf: String,

    // this how you opt-out of serialization of a member
    #[serde(skip)]
    is_running: bool,
    #[serde(skip)]
    value: u32,
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

    read_definitions: ModbusReadWriteDefinitions,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum RegisterType {
    Coils,
    Inputs,
    Holding,
}

struct MutexData {
    data: Vec<u16>,
    new_device_config: Option<DeviceConfig>,
    new_modbus_config: Option<ModbusReadWriteDefinitions>,
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
#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct SerialConfig {
    port: String,
    baudrate: Baudrate,
    slave: usize,
    parity: Parity,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
struct ModbusReadWriteDefinitions {
    register_type: RegisterType,
    start_address: u16,
    register_count: u16,
}
#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug, Clone)]
enum Protocol {
    Tcp,
    Rtu,
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
impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "TCP"),
            Protocol::Rtu => write!(f, "Serial"),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
enum DeviceConfig {
    Tcp,
    Serial(SerialConfig),
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            address_buf: "1".to_string(),
            value: 0,
            is_running: false,
            enable_register_edit: true,
            is_apply_clicked: false,
            enable_device_edit: true,
            mutex: Arc::new(Mutex::new(MutexData {
                data: Vec::new(),
                new_device_config: None,
                new_modbus_config: None,
            })),
            protocol: Protocol::Rtu,
            device_config: DeviceConfig::Serial(SerialConfig {
                port: "".to_string(),
                baudrate: Baudrate::Baud38400,
                slave: 1,
                parity: Parity::NoneParity,
            }),
            read_definitions: ModbusReadWriteDefinitions {
                register_type: RegisterType::Holding,
                start_address: 0,
                register_count: 5,
            },
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using

        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "custom_font".to_owned(),
            egui::FontData::from_static(include_bytes!("../assets/inter.otf")),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "custom_font".to_owned());

        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::variants::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }
}

impl eframe::App for TemplateApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Self {
            address_buf,
            value,
            is_running,
            enable_register_edit,
            is_apply_clicked,
            enable_device_edit,
            mutex,
            protocol,
            device_config,
            read_definitions,
        } = self;

        // Examples of how to create different panels and windows.
        // Pick whichever suits you.
        // Tip: a good default choice is to just keep the `CentralPanel`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

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

        egui::SidePanel::right("side_panel").show(ctx, |ui| {
            ui.label(format!(
                "{} Device Configuration",
                egui_phosphor::regular::GEAR_SIX
            ));

            ui.horizontal(|ui| {
                ComboBox::from_label("Protocol")
                    .selected_text(format!("{}", protocol))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(protocol, Protocol::Tcp, "TCP");
                        ui.selectable_value(protocol, Protocol::Rtu, "Serial RTU");
                    });
            });

            match protocol {
                Protocol::Tcp => {}
                Protocol::Rtu => {
                    // Modbus serial UI

                    ui.group(|ui| {
                        ui.set_enabled(*enable_device_edit);
                        modbus_serial_ui(device_config, ui);
                    });
                }
            }

            ui.separator();
            ui.group(|ui| {
                ui.set_enabled(*is_apply_clicked || !*is_running);
                ui.label(format!(
                    "{} Request Options",
                    egui_phosphor::regular::WRENCH
                ));

                ComboBox::from_label("Register Type")
                    .selected_text(format!("{}", read_definitions.register_type))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut read_definitions.register_type,
                            RegisterType::Coils,
                            "Coils",
                        );
                        ui.selectable_value(
                            &mut read_definitions.register_type,
                            RegisterType::Inputs,
                            "Input registers",
                        );
                        ui.selectable_value(
                            &mut read_definitions.register_type,
                            RegisterType::Holding,
                            "Holding registers",
                        );
                    });

                ui.horizontal(|ui| {
                    ui.label("Address");
                    (ui.text_edit_singleline(address_buf));
                });
                ui.add(
                    Slider::new(&mut read_definitions.register_count, 1..=1000).text("Quantity"),
                );

                if let Ok(address) = address_buf.parse::<u16>() {
                    read_definitions.start_address = address;
                } else {
                    ui.colored_label(Color32::DARK_RED, "Non valid address.");
                }
            });

            ui.separator();
            // let button = Button::new(format!("{} Connect", egui_phosphor::regular::PLUGS))
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !*is_running,
                        Button::new(format!("{} Connect", egui_phosphor::regular::PLUGS)),
                    )
                    //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                    .clicked()
                {
                    *enable_device_edit = false;
                    *enable_register_edit = true;
                    *is_running = true;
                    let mutex = Arc::clone(&mutex);
                    spawn_polling_thread(device_config, read_definitions, mutex);
                }
                if !*is_apply_clicked {
                    if ui
                        .add_enabled(
                            *enable_register_edit && *is_running,
                            Button::new(format!("{} Edit", egui_phosphor::regular::PEN)),
                        )
                        //.button(format!("{} Connect", egui_phosphor::regular::PLUGS))
                        .clicked()
                    {
                        *is_apply_clicked = true;
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
                        mutex.lock().new_modbus_config = Some(read_definitions.clone());
                        *is_apply_clicked = false;
                    }
                }
            });

            if let Some(data) = mutex.try_lock() {
                for val in data.data.iter() {
                    ui.label(format!("{}", val));
                }
            }

            //ui.colored_label(Color32::GREEN, format!("Value: {}", value));

            // ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            //     ui.horizontal(|ui| {
            //         ui.spacing_mut().item_spacing.x = 0.0;
            //         ui.label("powered by ");
            //         ui.hyperlink_to("egui", "https://github.com/emilk/egui");
            //         ui.label(" and ");
            //         ui.hyperlink_to(
            //             "eframe",
            //             "https://github.com/emilk/egui/tree/master/crates/eframe",
            //         );
            //         ui.label(".");
            //     });
            // });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.heading("eframe template");
            ui.hyperlink("https://github.com/emilk/eframe_template");
            ui.add(egui::github_link_file!(
                "https://github.com/emilk/eframe_template/blob/master/",
                "Source code."
            ));
            egui::warn_if_debug_build(ui);
        });

        if false {
            egui::Window::new("Window").show(ctx, |ui| {
                ui.label("Windows can be moved by dragging them.");
                ui.label("They are automatically sized based on contents.");
                ui.label("You can turn on resizing and scrolling if you like.");
                ui.label("You would normally choose either panels OR windows.");
            });
        }
    }
}

fn spawn_polling_thread(
    device_config: &mut DeviceConfig,
    read_definitions: &mut ModbusReadWriteDefinitions,
    mutex: Arc<Mutex<MutexData>>,
) {
    match device_config {
        DeviceConfig::Serial(config) => {
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
            let config = config.clone();
            let mut read_definitions = read_definitions.clone();
            thread::spawn(move || {
                let serial = serialport::new(config.port, baudrate_match)
                    .parity(parity)
                    .timeout(Duration::from_millis(1500));
                let ctx = connect_slave(&serial, Slave(config.slave as u8));
                if let Ok(mut ctx) = ctx {
                    loop {
                        thread::sleep(Duration::from_millis(100));
                        if let Some(mut mutex) = mutex.try_lock() {
                            if let Some(new_modbus_config) = mutex.new_modbus_config.clone() {
                                read_definitions = new_modbus_config;
                                mutex.new_device_config = None;

                                // mutex.lock().new_modbus_config = None;
                            }
                        }

                        match read_definitions.register_type {
                            RegisterType::Coils => {}
                            RegisterType::Inputs => {
                                let result = ctx.read_input_registers(
                                    read_definitions.start_address,
                                    read_definitions.register_count,
                                );
                                if let Ok(res) = result {
                                    let mut data = mutex.lock();
                                    data.data = res;
                                }
                            }
                            RegisterType::Holding => {
                                let result = ctx.read_holding_registers(
                                    read_definitions.start_address,
                                    read_definitions.register_count,
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

fn modbus_serial_ui(device_config: &mut DeviceConfig, ui: &mut egui::Ui) {
    match device_config {
        DeviceConfig::Serial(config) => {
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
                            //ui.selectable_value(config.port, port, port);
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
        }
        _ => {}
    }
}
