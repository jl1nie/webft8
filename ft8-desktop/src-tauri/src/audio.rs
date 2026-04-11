use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

const SAMPLE_RATE: u32 = 48000;
const TARGET_RATE: u32 = 12000;
const DECIM_RATIO: usize = (SAMPLE_RATE / TARGET_RATE) as usize; // 4
const PERIOD_SECS: usize = 15;
const BUFFER_SIZE: usize = TARGET_RATE as usize * PERIOD_SECS; // 180000

/// Audio device info returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub index: usize,
}

/// Shared audio state
pub struct AudioState {
    /// 12 kHz snapshot buffer for decode
    pub snapshot_buf: Arc<Mutex<Vec<f32>>>,
    /// Current write position in snapshot buffer
    pub write_pos: Arc<Mutex<usize>>,
    /// Flag: buffer is full (15 seconds captured)
    pub buffer_full: Arc<AtomicBool>,
    /// Flag: audio is recording
    pub recording: Arc<AtomicBool>,
    /// Peak level (0.0 - 1.0)
    pub peak_level: Arc<Mutex<f32>>,
    /// Active stream handle
    stream: Mutex<Option<cpal::Stream>>,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            snapshot_buf: Arc::new(Mutex::new(vec![0.0f32; BUFFER_SIZE])),
            write_pos: Arc::new(Mutex::new(0)),
            buffer_full: Arc::new(AtomicBool::new(false)),
            recording: Arc::new(AtomicBool::new(false)),
            peak_level: Arc::new(Mutex::new(0.0)),
            stream: Mutex::new(None),
        }
    }

    /// Take a snapshot of current buffer and reset
    pub fn take_snapshot(&self) -> (Vec<f32>, u32) {
        let mut pos = self.write_pos.lock().unwrap();
        let buf = self.snapshot_buf.lock().unwrap();
        let samples = buf[..*pos].to_vec();
        *pos = 0;
        self.buffer_full.store(false, Ordering::Relaxed);
        (samples, TARGET_RATE)
    }
}

/// List available audio input devices
#[tauri::command]
pub fn audio_list_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let mut devices = Vec::new();
    if let Ok(devs) = host.input_devices() {
        for (i, dev) in devs.enumerate() {
            let name = dev.name().unwrap_or_else(|_| format!("Device {}", i));
            devices.push(AudioDeviceInfo { name, index: i });
        }
    }
    devices
}

/// Start audio capture from specified device
#[tauri::command]
pub fn audio_start(
    state: tauri::State<'_, AudioState>,
    device_index: usize,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host
        .input_devices()
        .map_err(|e| e.to_string())?
        .nth(device_index)
        .ok_or("Device not found")?;

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let snapshot_buf = state.snapshot_buf.clone();
    let write_pos = state.write_pos.clone();
    let buffer_full = state.buffer_full.clone();
    let recording = state.recording.clone();
    let peak_level = state.peak_level.clone();

    // Boxcar decimation state
    let mut decim_accum: f32 = 0.0;
    let mut decim_count: usize = 0;

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !recording.load(Ordering::Relaxed) {
                    return;
                }

                // Track peak
                let mut peak = 0.0f32;
                for &s in data {
                    let abs = s.abs();
                    if abs > peak {
                        peak = abs;
                    }
                }
                if let Ok(mut p) = peak_level.lock() {
                    if peak > *p {
                        *p = peak;
                    }
                }

                // Decimate 48kHz → 12kHz via boxcar average
                let mut buf = snapshot_buf.lock().unwrap();
                let mut pos = write_pos.lock().unwrap();

                for &sample in data {
                    decim_accum += sample;
                    decim_count += 1;
                    if decim_count >= DECIM_RATIO {
                        let avg = decim_accum / DECIM_RATIO as f32;
                        decim_accum = 0.0;
                        decim_count = 0;

                        if *pos < BUFFER_SIZE {
                            buf[*pos] = avg;
                            *pos += 1;
                        }
                        if *pos >= BUFFER_SIZE {
                            buffer_full.store(true, Ordering::Relaxed);
                        }
                    }
                }
            },
            |err| {
                log::error!("Audio stream error: {}", err);
            },
            None,
        )
        .map_err(|e| format!("Failed to build audio stream: {}", e))?;

    stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;

    state.recording.store(true, Ordering::Relaxed);
    *state.write_pos.lock().unwrap() = 0;
    state.buffer_full.store(false, Ordering::Relaxed);

    let mut s = state.stream.lock().unwrap();
    *s = Some(stream);

    log::info!("Audio capture started: {} @ {} Hz → {} Hz",
        device.name().unwrap_or_default(), SAMPLE_RATE, TARGET_RATE);
    Ok(())
}

/// Stop audio capture
#[tauri::command]
pub fn audio_stop(state: tauri::State<'_, AudioState>) -> Result<(), String> {
    state.recording.store(false, Ordering::Relaxed);
    let mut s = state.stream.lock().unwrap();
    *s = None; // Drop stream stops capture
    log::info!("Audio capture stopped");
    Ok(())
}

/// Get current peak level and reset
#[tauri::command]
pub fn audio_get_peak(state: tauri::State<'_, AudioState>) -> f32 {
    let mut p = state.peak_level.lock().unwrap();
    let val = *p;
    *p = 0.0;
    val
}

/// Check if buffer is full (15 seconds captured)
#[tauri::command]
pub fn audio_buffer_ready(state: tauri::State<'_, AudioState>) -> bool {
    state.buffer_full.load(Ordering::Relaxed)
}

/// Get snapshot and sample rate for decode
#[tauri::command]
pub fn audio_get_snapshot(state: tauri::State<'_, AudioState>) -> (Vec<f32>, u32) {
    state.take_snapshot()
}
