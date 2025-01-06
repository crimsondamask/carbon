#![warn(clippy::all, rust_2018_idioms)]

mod app;
mod components;
mod modbus;
mod mutex_data;
pub use app::CarbonApp;
pub use modbus::*;
