// SPDX-FileCopyrightText: 2025 LunNova
// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use color_eyre::eyre::{Context, Result};
use evdev_rs::{
	AbsInfo, Device, DeviceWrapper, EnableCodeData, UInputDevice, UninitDevice,
	enums::{EventCode, EventType, int_to_event_type, int_to_input_prop},
	util::{EventCodeIterator, EventTypeIterator, InputPropIterator, event_code_to_int, int_to_event_code},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

use crate::{DeviceInfo, OutputDeviceConfig, print_device_info};

/// evdev doesn't expose all key codes via iterator, so scan up to this value
const MAX_KEY_CODE_SCAN: u32 = 1024;

/// Serializable version of AbsInfo for device profiles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableAbsInfo {
	pub value: i32,
	pub minimum: i32,
	pub maximum: i32,
	pub fuzz: i32,
	pub flat: i32,
	pub resolution: i32,
}

impl From<AbsInfo> for SerializableAbsInfo {
	fn from(abs_info: AbsInfo) -> Self {
		Self {
			value: abs_info.value,
			minimum: abs_info.minimum,
			maximum: abs_info.maximum,
			fuzz: abs_info.fuzz,
			flat: abs_info.flat,
			resolution: abs_info.resolution,
		}
	}
}

impl From<SerializableAbsInfo> for AbsInfo {
	fn from(ser_info: SerializableAbsInfo) -> Self {
		Self {
			value: ser_info.value,
			minimum: ser_info.minimum,
			maximum: ser_info.maximum,
			fuzz: ser_info.fuzz,
			flat: ser_info.flat,
			resolution: ser_info.resolution,
		}
	}
}

/// Device capability profile that can be saved/loaded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
	/// Profile format version for compatibility
	pub version: u32,
	/// Device information
	pub device_info: DeviceInfo,
	/// Supported event types (as raw u32 values)
	pub event_types: Vec<u32>,
	/// Supported event codes (as raw u32 type, u32 code pairs)
	pub event_codes: Vec<(u32, u32)>,
	/// AbsInfo for absolute axes (type,code -> AbsInfo mapping)
	pub abs_info: BTreeMap<String, SerializableAbsInfo>,
	/// REP event values (type,code -> value mapping)
	pub rep_info: BTreeMap<String, i32>,
	/// Supported input properties (as raw u32 values)
	pub input_properties: Vec<u32>,
	/// Timestamp when profile was created
	pub created_at: String,
}

impl DeviceProfile {
	/// Create a profile by scanning a connected device
	pub fn from_device(device: &Device) -> Result<Self> {
		let mut event_types = Vec::new();
		let mut event_codes = Vec::new();
		let mut abs_info = BTreeMap::new();
		let mut rep_info = BTreeMap::new();
		let mut input_properties = Vec::new();

		for event_type in EventTypeIterator::new() {
			if device.has_event_type(&event_type) {
				event_types.push(event_type as u32);

				// Use iterator for known codes
				for event_code in EventCodeIterator::new(&event_type) {
					if device.has_event_code(&event_code) {
						let (type_raw, code_raw) = event_code_to_int(&event_code);
						event_codes.push((type_raw, code_raw));

						if let EventCode::EV_ABS(_) = event_code
							&& let Some(abs) = device.abs_info(&event_code)
						{
							let key = format!("{type_raw}_{code_raw}");
							abs_info.insert(key, abs.into());
						} else if let EventCode::EV_REP(_) = event_code
							&& let Some(value) = device.event_value(&event_code)
						{
							let key = format!("{type_raw}_{code_raw}");
							rep_info.insert(key, value);
						}
					}
				}

				// Scan for unknown codes that the iterator missed (only for EV_KEY type)
				if event_type == EventType::EV_KEY {
					let type_raw = event_type as u32;
					for code_raw in 0..MAX_KEY_CODE_SCAN {
						let event_code = int_to_event_code(type_raw, code_raw);
						if let EventCode::EV_UNK { .. } = event_code {
							if device.has_event_code(&event_code) && !event_codes.contains(&(type_raw, code_raw)) {
								event_codes.push((type_raw, code_raw));
							}
						}
					}
				}
			}
		}

		for prop in InputPropIterator::new() {
			if device.has_property(&prop) {
				input_properties.push(prop as u32);
			}
		}

		Ok(Self {
			version: 1,
			device_info: DeviceInfo::from_evdev_device(device),
			event_types,
			event_codes,
			abs_info,
			rep_info,
			input_properties,
			created_at: chrono::Utc::now().to_rfc3339(),
		})
	}

	/// Save profile to a file
	pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
		let content = serde_json::to_string_pretty(self).context("Failed to serialize device profile")?;

		std::fs::write(path.as_ref(), content).with_context(|| format!("Failed to write profile to {}", path.as_ref().display()))?;

		Ok(())
	}

	/// Load profile from a file
	pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
		let content =
			std::fs::read_to_string(path.as_ref()).with_context(|| format!("Failed to read profile from {}", path.as_ref().display()))?;

		let profile: DeviceProfile =
			serde_json::from_str(&content).with_context(|| format!("Failed to parse profile from {}", path.as_ref().display()))?;

		Ok(profile)
	}

	/// Apply this profile's capabilities to a UninitDevice
	pub fn apply_to_device(&self, device: &UninitDevice) -> Result<()> {
		for &event_type_raw in &self.event_types {
			if let Some(event_type) = int_to_event_type(event_type_raw) {
				device
					.enable_event_type(&event_type)
					.with_context(|| format!("Failed to enable event type {event_type:?}"))?;
			}
		}

		for &(type_raw, code_raw) in &self.event_codes {
			let event_code = int_to_event_code(type_raw, code_raw);

			// device.enable is recommended by the docs for all types, but as of evdev-rs 0.6.1 isn't suitable as it fails for abs and rep

			// Check if this is an absolute axis with stored AbsInfo
			if type_raw == EventType::EV_ABS as u32 {
				let key = format!("{type_raw}_{code_raw}");
				if let Some(ser_abs_info) = self.abs_info.get(&key) {
					let abs_info: AbsInfo = ser_abs_info.clone().into();
					device
						.enable_event_code(&event_code, Some(EnableCodeData::AbsInfo(abs_info)))
						.with_context(|| format!("Failed to enable abs event code {event_code:?}"))?;
				} else {
					device
						.enable_event_code(&event_code, None)
						.with_context(|| format!("Failed to enable event code {event_code:?}"))?;
				}
			} else if type_raw == EventType::EV_REP as u32 {
				let key = format!("{type_raw}_{code_raw}");
				if let Some(&rep_value) = self.rep_info.get(&key) {
					device
						.enable_event_code(&event_code, Some(EnableCodeData::RepInfo(rep_value)))
						.with_context(|| format!("Failed to enable rep event code {event_code:?}"))?;
				} else {
					device
						.enable_event_code(&event_code, None)
						.with_context(|| format!("Failed to enable event code {event_code:?}"))?;
				}
			} else {
				device
					.enable_event_code(&event_code, None)
					.with_context(|| format!("Failed to enable event code {event_code:?}"))?;
			}
		}

		for &prop_raw in &self.input_properties {
			if let Some(prop) = int_to_input_prop(prop_raw) {
				device
					.enable_property(&prop)
					.with_context(|| format!("Failed to enable property {prop:?}"))?;
			}
		}

		for (key, ser_abs_info) in &self.abs_info {
			if let Some((type_raw, code_raw)) = key
				.split_once('_')
				.and_then(|(t, c)| Some((t.parse::<u32>().ok()?, c.parse::<u32>().ok()?)))
			{
				let event_code = int_to_event_code(type_raw, code_raw);
				let abs_info: AbsInfo = ser_abs_info.clone().into();
				device.set_abs_info(&event_code, &abs_info);
			}
		}

		for (key, &rep_value) in &self.rep_info {
			if let Some((type_raw, code_raw)) = key
				.split_once('_')
				.and_then(|(t, c)| Some((t.parse::<u32>().ok()?, c.parse::<u32>().ok()?)))
			{
				let event_code = int_to_event_code(type_raw, code_raw);
				let _ = device.set_event_value(&event_code, rep_value);
			}
		}

		Ok(())
	}
}

/// Create a virtual device from a saved profile (no physical device required)
pub fn create_virtual_device_from_profile(profile: &DeviceProfile, output_config: &OutputDeviceConfig) -> Result<UInputDevice> {
	// Create a new blank device
	let custom_device = UninitDevice::new().ok_or_else(|| color_eyre::eyre::eyre!("Failed to create UninitDevice"))?;

	// Set custom device identity
	custom_device.set_name(&output_config.name);
	custom_device.set_phys(&profile.device_info.phys);
	custom_device.set_uniq(&profile.device_info.uniq);

	// Set custom or default IDs
	let custom_vid = output_config.vendor_id.unwrap_or(profile.device_info.vendor_id);
	let custom_pid = output_config.product_id.unwrap_or(profile.device_info.product_id);
	let custom_version = output_config.version.unwrap_or(profile.device_info.version);
	let custom_bus_type = output_config.bus_type.unwrap_or(profile.device_info.bus_type);

	custom_device.set_vendor_id(custom_vid);
	custom_device.set_product_id(custom_pid);
	custom_device.set_version(custom_version);
	custom_device.set_bustype(custom_bus_type);

	// Apply all capabilities from the profile
	profile.apply_to_device(&custom_device)?;

	// Create the virtual device from our custom device
	let output = UInputDevice::create_from_device(&custom_device).context("creating virtual device from profile")?;

	// Print diagnostic info about the created device
	println!("Virtual device created successfully:");

	// Create a Device from the UInputDevice to read back properties
	let device_path = output.devnode().unwrap();
	let device_for_reading = Device::new_from_path(device_path).context("creating Device from UInputDevice for diagnostics")?;

	print_device_info(&device_for_reading);

	Ok(output)
}

/// Create profile filename from VID/PID/Version
pub fn format_profile_filename(vid: u16, pid: u16, version: u16) -> String {
	format!("profiles/{vid:04x}_{pid:04x}_{version:04x}.json")
}

/// Save all device profiles for available devices
pub fn save_all_profiles() -> Result<()> {
	println!("Saving device profiles for all available devices...");
	let devices = crate::DeviceInfo::obtain_device_list()?;

	for device_info in devices {
		println!("Scanning device: {}", device_info.name);

		// Use scoped block to ensure device handle is properly closed
		let profile_result = {
			let path = device_info
				.path
				.as_ref()
				.ok_or_else(|| color_eyre::eyre::eyre!("Device missing path"))?;
			match Device::new_from_path(path)
				.with_context(|| format!("failed to create Device from {}", path.display()))
				.and_then(|device| DeviceProfile::from_device(&device))
			{
				Ok(profile) => Ok(profile),
				Err(e) => {
					eprintln!("  → Failed to scan device: {e}");
					Err(e)
				}
			}
		};

		if let Ok(profile) = profile_result {
			let filename = format_profile_filename(device_info.vendor_id, device_info.product_id, device_info.version);

			std::fs::create_dir_all("profiles")?;

			profile.save_to_file(&filename)?;
			println!("  → Saved profile to {filename}");
		}
	}

	println!("Profile saving complete!");
	Ok(())
}
