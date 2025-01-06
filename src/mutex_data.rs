use crate::app::{DeviceConfigUiBuffer, S7Data, S7MessageTag};
//#################################################### The Mutex used between
//the main and background threads.
pub struct MutexData {
    pub data: Vec<u16>,
    pub s7_read_data: S7Data,
    pub s7_message: Option<S7MessageTag>,
    pub achieved_scan_time: u128,
    pub new_config: Option<DeviceConfigUiBuffer>,
    pub kill_thread: bool,
}
