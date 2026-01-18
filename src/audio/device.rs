//! Audio device enumeration and management

use cpal::traits::{DeviceTrait, HostTrait};
use crate::error::AudioError;
use crate::protocol::AudioDeviceInfo;

/// Wrapper around cpal device
pub struct AudioDevice {
    inner: cpal::Device,
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
}

impl AudioDevice {
    pub fn from_cpal(device: cpal::Device, is_input: bool, is_output: bool) -> Self {
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        Self {
            inner: device,
            name,
            is_input,
            is_output,
        }
    }
    
    pub fn inner(&self) -> &cpal::Device {
        &self.inner
    }
    
    pub fn into_inner(self) -> cpal::Device {
        self.inner
    }
    
    /// Get supported input configurations
    pub fn supported_input_configs(&self) -> Result<Vec<cpal::SupportedStreamConfigRange>, AudioError> {
        self.inner
            .supported_input_configs()
            .map(|iter| iter.collect())
            .map_err(|e| AudioError::DeviceNotFound(e.to_string()))
    }
    
    /// Get supported output configurations
    pub fn supported_output_configs(&self) -> Result<Vec<cpal::SupportedStreamConfigRange>, AudioError> {
        self.inner
            .supported_output_configs()
            .map(|iter| iter.collect())
            .map_err(|e| AudioError::DeviceNotFound(e.to_string()))
    }
    
    /// Get default input config
    pub fn default_input_config(&self) -> Result<cpal::SupportedStreamConfig, AudioError> {
        self.inner
            .default_input_config()
            .map_err(|e| AudioError::DeviceNotFound(e.to_string()))
    }
    
    /// Get default output config
    pub fn default_output_config(&self) -> Result<cpal::SupportedStreamConfig, AudioError> {
        self.inner
            .default_output_config()
            .map_err(|e| AudioError::DeviceNotFound(e.to_string()))
    }
}

/// List all available audio devices
pub fn list_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let mut devices = Vec::new();
    
    // Get default devices
    let default_input_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    let default_output_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());
    
    // Input devices
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                let id = format!("input:{}", name);
                let is_default = default_input_name.as_ref() == Some(&name);
                
                let (sample_rates, channels) = get_device_capabilities(&device, true);
                
                devices.push(AudioDeviceInfo {
                    id,
                    name: name.clone(),
                    is_input: true,
                    is_output: false,
                    is_default,
                    sample_rates,
                    channels,
                });
            }
        }
    }
    
    // Output devices
    if let Ok(output_devices) = host.output_devices() {
        for device in output_devices {
            if let Ok(name) = device.name() {
                let id = format!("output:{}", name);
                let is_default = default_output_name.as_ref() == Some(&name);
                
                let (sample_rates, channels) = get_device_capabilities(&device, false);
                
                // Check if we already have this device as input
                if let Some(existing) = devices.iter_mut().find(|d| d.name == name) {
                    existing.is_output = true;
                    if is_default && !existing.is_default {
                        existing.is_default = true;
                    }
                } else {
                    devices.push(AudioDeviceInfo {
                        id,
                        name,
                        is_input: false,
                        is_output: true,
                        is_default,
                        sample_rates,
                        channels,
                    });
                }
            }
        }
    }
    
    devices
}

/// Get device capabilities
fn get_device_capabilities(device: &cpal::Device, is_input: bool) -> (Vec<u32>, Vec<u16>) {
    let mut sample_rates = Vec::new();
    let mut channels = Vec::new();
    
    let process_configs = |configs: Box<dyn Iterator<Item = cpal::SupportedStreamConfigRange>>| {
        let mut rates = Vec::new();
        let mut chans = Vec::new();
        for config in configs {
            // Common sample rates
            for rate_val in [44100u32, 48000, 88200, 96000, 176400, 192000] {
                let rate = cpal::SampleRate(rate_val);
                if rate >= config.min_sample_rate() && rate <= config.max_sample_rate() {
                    if !rates.contains(&rate_val) {
                        rates.push(rate_val);
                    }
                }
            }
            
            let ch = config.channels();
            if !chans.contains(&ch) {
                chans.push(ch);
            }
        }
        (rates, chans)
    };
    
    if is_input {
        if let Ok(configs) = device.supported_input_configs() {
            let (rates, chans) = process_configs(Box::new(configs));
            sample_rates = rates;
            channels = chans;
        }
    } else {
        if let Ok(configs) = device.supported_output_configs() {
            let (rates, chans) = process_configs(Box::new(configs));
            sample_rates = rates;
            channels = chans;
        }
    }
    
    sample_rates.sort();
    channels.sort();
    
    (sample_rates, channels)
}

/// Get a device by its ID
pub fn get_device_by_id(id: &str) -> Result<AudioDevice, AudioError> {
    let host = cpal::default_host();
    
    // Parse device type from ID
    let (device_type, name) = if let Some(name) = id.strip_prefix("input:") {
        ("input", name)
    } else if let Some(name) = id.strip_prefix("output:") {
        ("output", name)
    } else {
        // Assume input for backward compatibility
        ("input", id)
    };
    
    let devices = match device_type {
        "input" => host.input_devices(),
        "output" => host.output_devices(),
        _ => return Err(AudioError::DeviceNotFound(id.to_string())),
    };
    
    let devices = devices.map_err(|e| AudioError::DeviceNotFound(e.to_string()))?;
    
    for device in devices {
        if let Ok(device_name) = device.name() {
            if device_name == name {
                return Ok(AudioDevice::from_cpal(
                    device,
                    device_type == "input",
                    device_type == "output",
                ));
            }
        }
    }
    
    Err(AudioError::DeviceNotFound(id.to_string()))
}

/// Get default input device
pub fn get_default_input_device() -> Result<AudioDevice, AudioError> {
    let host = cpal::default_host();
    host.default_input_device()
        .map(|d| AudioDevice::from_cpal(d, true, false))
        .ok_or_else(|| AudioError::DeviceNotFound("No default input device".to_string()))
}

/// Get default output device
pub fn get_default_output_device() -> Result<AudioDevice, AudioError> {
    let host = cpal::default_host();
    host.default_output_device()
        .map(|d| AudioDevice::from_cpal(d, false, true))
        .ok_or_else(|| AudioError::DeviceNotFound("No default output device".to_string()))
}

#[cfg(target_os = "windows")]
pub mod wasapi {
    //! WASAPI-specific device handling
    //!
    //! For low-latency audio on Windows, we can use WASAPI in either:
    //! - Shared mode: Lower latency than MME/DirectSound, allows multiple apps
    //! - Exclusive mode: Lowest latency, but exclusive access to device
    
    /// WASAPI mode configuration
    #[derive(Debug, Clone, Copy)]
    pub enum WasapiMode {
        /// Shared mode (default)
        Shared,
        /// Exclusive mode for lowest latency
        Exclusive,
    }
    
    /// Check if WASAPI is available
    pub fn is_available() -> bool {
        // cpal uses WASAPI by default on Windows
        cfg!(target_os = "windows")
    }
    
    /// Get WASAPI-specific host
    pub fn get_wasapi_host() -> Option<cpal::Host> {
        #[cfg(target_os = "windows")]
        {
            // cpal's default host on Windows is WASAPI
            Some(cpal::default_host())
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }
}
