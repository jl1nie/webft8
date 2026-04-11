use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::Mutex;
use std::time::Duration;

/// Shared serial port state managed by Tauri
pub struct SerialState {
    port: Mutex<Option<Box<dyn serialport::SerialPort>>>,
}

impl SerialState {
    pub fn new() -> Self {
        Self {
            port: Mutex::new(None),
        }
    }
}

/// Serial port info returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
}

/// List available serial ports
#[tauri::command]
pub fn serial_list_ports() -> Vec<PortInfo> {
    match serialport::available_ports() {
        Ok(ports) => ports
            .into_iter()
            .map(|p| {
                let (vid, pid) = match &p.port_type {
                    serialport::SerialPortType::UsbPort(usb) => {
                        (usb.vid, usb.pid)
                    }
                    _ => (0, 0),
                };
                PortInfo {
                    name: p.port_name,
                    vid,
                    pid,
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Open a serial port
#[tauri::command]
pub fn serial_open(
    state: tauri::State<'_, SerialState>,
    port_name: String,
    baud_rate: u32,
    stop_bits: Option<u8>,
) -> Result<(), String> {
    let mut guard = state.port.lock().map_err(|e| e.to_string())?;

    // Close existing port if any
    if guard.is_some() {
        *guard = None;
    }

    let sb = match stop_bits.unwrap_or(1) {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    };

    let port = serialport::new(&port_name, baud_rate)
        .stop_bits(sb)
        .parity(serialport::Parity::None)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Failed to open {}: {}", port_name, e))?;

    log::info!("Serial port opened: {} @ {} baud, stop_bits={:?}", port_name, baud_rate, sb);
    *guard = Some(port);
    Ok(())
}

/// Write bytes to the serial port
#[tauri::command]
pub fn serial_write(
    state: tauri::State<'_, SerialState>,
    data: Vec<u8>,
) -> Result<usize, String> {
    let mut guard = state.port.lock().map_err(|e| e.to_string())?;
    let port = guard.as_mut().ok_or("Serial port not open")?;
    let n = port.write(&data).map_err(|e| format!("Write failed: {}", e))?;
    port.flush().map_err(|e| format!("Flush failed: {}", e))?;
    Ok(n)
}

/// Close the serial port
#[tauri::command]
pub fn serial_close(state: tauri::State<'_, SerialState>) -> Result<(), String> {
    let mut guard = state.port.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        *guard = None;
        log::info!("Serial port closed");
    }
    Ok(())
}
