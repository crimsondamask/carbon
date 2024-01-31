#![warn(clippy::all, rust_2018_idioms)]

mod app;
mod modbus;
pub use app::CarbonApp;
pub use modbus::*;
