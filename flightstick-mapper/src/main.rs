// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

pub mod profile;
pub mod rgb;
use color_eyre::eyre::{Context, Result, bail};
use evdev_rs::{
	Device, DeviceWrapper, GrabMode, InputEvent, ReadFlag, ReadStatus, UInputDevice,
	enums::{EV_ABS, EventCode, EventType},
	util::{EventCodeIterator, EventTypeIterator, event_code_to_int},
};

use profile::{DeviceProfile, create_virtual_device_from_profile, format_profile_filename, save_all_profiles};
use serde::{Deserialize, Serialize};
use std::{
	collections::HashMap,
	fmt,
	io::Read,
	os::unix::io::AsRawFd,
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread,
	time::Duration,
};

/// Device identification method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeviceSelector {
	/// Match by device name only
	Name(String),
	/// Match by name and physical address
	NameAndPhys { name: String, phys: String },
	/// Match by VID/PID/Version
	VidPidVersion { vid: u16, pid: u16, version: u16 },
	/// Match by name with VID/PID/Version for disambiguation
	NameWithIds { name: String, vid: u16, pid: u16, version: u16 },
}

/// Configuration for the output virtual device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDeviceConfig {
	/// Custom name for the virtual device
	pub name: String,
	/// Custom vendor ID (optional, defaults to original + offset)
	pub vendor_id: Option<u16>,
	/// Custom product ID (optional, defaults to original + offset)
	pub product_id: Option<u16>,
	/// Custom version (optional, defaults to original)
	pub version: Option<u16>,
	/// Custom bus type (optional, defaults to original device bus type)
	pub bus_type: Option<u16>,
}

/// Configuration for a single device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
	/// Device identification method
	pub device: DeviceSelector,
	/// Human-readable name for this device instance
	pub name: String,
	/// Axis mappings for this device
	pub axes: HashMap<String, AxisConfig>,
	/// Whether to enable device on startup
	#[serde(default = "default_enabled")]
	pub enabled: bool,
	/// Configuration for the output virtual device
	pub output_device: Option<OutputDeviceConfig>,
}

fn default_enabled() -> bool {
	true
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
	/// List of devices to manage
	pub devices: Vec<DeviceConfig>,
}

impl Config {
	/// Load configuration from a TOML file
	pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
		let content =
			std::fs::read_to_string(path.as_ref()).with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

		let config: Config = toml::from_str(&content).with_context(|| format!("Failed to parse config file: {}", path.as_ref().display()))?;

		Ok(config)
	}

	/// Save configuration to a TOML file
	pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
		let content = toml::to_string_pretty(self).context("Failed to serialize config to TOML")?;

		std::fs::write(path.as_ref(), content).with_context(|| format!("Failed to write config file: {}", path.as_ref().display()))?;

		Ok(())
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
	pub name: String,
	/// Path to device file (for physical devices) or profile file (for profile-based devices)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub path: Option<PathBuf>,
	pub phys: String,
	pub uniq: String,
	pub vendor_id: u16,
	pub product_id: u16,
	pub version: u16,
	pub bus_type: u16,
}

impl fmt::Display for DeviceInfo {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{} (VID={:04x}/PID={:04x}/Version={:04x})",
			self.name, self.vendor_id, self.product_id, self.version
		)
	}
}

impl DeviceInfo {
	pub fn with_path(path: PathBuf) -> Result<Self> {
		let input = Device::new_from_path(&path).with_context(|| format!("failed to create Device from {}", path.display()))?;

		let mut device_info = Self::from_evdev_device(&input);
		device_info.path = Some(path);
		Ok(device_info)
	}

	pub fn from_evdev_device(device: &Device) -> Self {
		DeviceInfo {
			name: device.name().unwrap_or("").to_string(),
			phys: device.phys().unwrap_or("").to_string(),
			uniq: device.uniq().unwrap_or("").to_string(),
			vendor_id: device.vendor_id(),
			product_id: device.product_id(),
			version: device.version(),
			bus_type: device.bustype(),
			path: None,
		}
	}

	pub fn with_name(name: &str, phys: Option<&str>, device_ids: Option<(u16, u16, u16)>) -> Result<Self> {
		let mut devices = Self::obtain_device_list()?;

		if let Some((vid, pid, version)) = device_ids {
			match devices
				.iter()
				.position(|item| item.vendor_id == vid && item.product_id == pid && item.version == version)
			{
				Some(idx) => return Ok(devices.remove(idx)),
				None => {
					// Try to fall back to a saved profile
					let profile_filename = format_profile_filename(vid, pid, version);
					if std::path::Path::new(&profile_filename).exists() {
						// Load the profile and create a synthetic DeviceInfo
						let profile = DeviceProfile::load_from_file(&profile_filename)
							.with_context(|| format!("Failed to load profile from {profile_filename}"))?;

						let mut device_info = profile.device_info.clone();
						device_info.path = Some(PathBuf::from(&profile_filename)); // Store profile path
						return Ok(device_info);
					}

					bail!(
						"Requested device with VID={:04x}/PID={:04x}/Version={:04x} was not found (no physical device or saved profile)",
						vid,
						pid,
						version
					);
				}
			}
		}

		if let Some(phys) = phys {
			match devices.iter().position(|item| item.phys == phys) {
				Some(idx) => return Ok(devices.remove(idx)),
				None => {
					bail!("Requested device `{}` with phys=`{}` was not found", name, phys);
				}
			}
		}

		let mut devices_with_name: Vec<_> = devices.into_iter().filter(|item| item.name == name).collect();

		if devices_with_name.is_empty() {
			bail!("No device found with name `{}`", name);
		}

		if devices_with_name.len() > 1 {
			eprintln!("Multiple devices match name `{name}`, using first entry:");
			for dev in &devices_with_name {
				eprintln!("  {dev}");
			}
		}

		Ok(devices_with_name.remove(0))
	}

	fn obtain_device_list() -> Result<Vec<DeviceInfo>> {
		let mut devices = vec![];
		for entry in std::fs::read_dir("/dev/input")? {
			let entry = entry?;

			if !entry.file_name().to_str().unwrap_or("").starts_with("event") {
				continue;
			}
			let path = entry.path();
			if path.is_dir() {
				continue;
			}

			match DeviceInfo::with_path(path) {
				Ok(item) => devices.push(item),
				Err(err) => eprintln!("{err:#}"),
			}
		}

		fn event_number(path: &Path) -> u32 {
			path.file_name()
				.and_then(|n| n.to_str())
				.and_then(|s| s.strip_prefix("event"))
				.and_then(|n| n.parse().ok())
				.unwrap_or(u32::MAX)
		}

		devices.sort_by(|a, b| {
			a.name.cmp(&b.name).then_with(|| {
				let a_num = a.path.as_ref().map(|p| event_number(p)).unwrap_or(u32::MAX);
				let b_num = b.path.as_ref().map(|p| event_number(p)).unwrap_or(u32::MAX);
				a_num.cmp(&b_num)
			})
		});

		Ok(devices)
	}
}

/// NURBS curve configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurveConfig {
	/// Control points for the curve
	pub control_points: Vec<Vec<f64>>,
	/// Knot vector for the curve
	pub knots: Vec<f64>,
	/// Weights for the control points
	pub weights: Vec<f64>,
	/// Degree of the curve
	pub degree: usize,
}

/// Curve type for axis mapping
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CurveType {
	/// Simple polynomial curve: output = sign(input) * |input|^power
	#[serde(rename = "polynomial")]
	Polynomial {
		/// Power/exponent for the curve (2.0 = quadratic, 1.0 = linear)
		power: f64,
		/// Deadzone radius around center (0.0 to 1.0)
		#[serde(default)]
		deadzone: f64,
	},
	/// NURBS curve (not yet implemented)
	#[serde(rename = "nurbs")]
	Nurbs(CurveConfig),
}

/// Configuration for a single axis remapping
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxisConfig {
	/// Curve to apply to this axis. If None, values pass through unchanged
	#[serde(default)]
	pub curve: Option<CurveType>,
}

/// Print diagnostic information about a device
fn print_device_info(device: &Device) {
	println!("  Name: '{}'", device.name().unwrap_or("<none>"));
	println!(
		"  VID: 0x{:04x}, PID: 0x{:04x}, Version: 0x{:04x}, Bus: 0x{:04x}",
		device.vendor_id(),
		device.product_id(),
		device.version(),
		device.bustype()
	);
	println!("  Phys: '{}'", device.phys().unwrap_or("<none>"));
	println!("  Uniq: '{}'", device.uniq().unwrap_or("<none>"));

	// Count actual axes by checking what the device reports
	let mut abs_axes = Vec::new();
	for event_type in EventTypeIterator::new() {
		if event_type == EventType::EV_ABS && device.has_event_type(&event_type) {
			for event_code in EventCodeIterator::new(&event_type) {
				if device.has_event_code(&event_code) {
					let (_, code_raw) = event_code_to_int(&event_code);
					abs_axes.push(format!("3_{}", code_raw));
				}
			}
		}
	}
	println!("  Absolute axes: {} ({})", abs_axes.len(), abs_axes.join(", "));

	// Count event types and codes from actual device
	let mut event_type_count = 0;
	let mut event_code_count = 0;
	for event_type in EventTypeIterator::new() {
		if device.has_event_type(&event_type) {
			event_type_count += 1;
			for event_code in EventCodeIterator::new(&event_type) {
				if device.has_event_code(&event_code) {
					event_code_count += 1;
				}
			}
		}
	}
	println!("  Event types: {} supported", event_type_count);
	println!("  Event codes: {} supported", event_code_count);
}

/// Set up device permissions (extracted utility function)
fn setup_device_permissions(device_path: &Path) -> Result<()> {
	use std::os::unix::fs::PermissionsExt;

	let metadata = std::fs::metadata(device_path).with_context(|| format!("Failed to get metadata for {}", device_path.display()))?;
	let mut perms = metadata.permissions();
	perms.set_mode(0o600); // Owner read/write only
	std::fs::set_permissions(device_path, perms).with_context(|| format!("Failed to set permissions on {}", device_path.display()))?;

	// Remove any ACLs if the platform supports it
	#[cfg(target_os = "linux")]
	{
		use std::process::Command;
		let output = Command::new("setfacl")
			.args(["-b", device_path.to_str().unwrap_or("")])
			.output()
			.with_context(|| "Failed to execute setfacl command")?;

		if !output.status.success() {
			eprintln!("Warning: Failed to remove ACLs: {}", String::from_utf8_lossy(&output.stderr));
		}
	}

	Ok(())
}

/// Consolidated device management - combines discovery, setup, event processing, and thread lifecycle
pub struct ManagedDevice {
	device_config: DeviceConfig,
	device_info: DeviceInfo,
	cached_capabilities: Option<DeviceProfile>,
	virtual_output: Option<UInputDevice>,
	axis_configs: HashMap<u16, AxisConfig>,
	running: Arc<AtomicBool>,
	clone_physical: bool,
}

impl ManagedDevice {
	/// Create a new managed device from configuration
	pub fn new(device_config: DeviceConfig, clone_physical: bool) -> Result<Self> {
		let device_info = Self::find_device_internal(&device_config.device)?;
		let is_profile = device_info.path.as_ref().and_then(|p| p.extension()).and_then(|s| s.to_str()) == Some("json");

		let cached_capabilities = if is_profile {
			let path = device_info
				.path
				.as_ref()
				.ok_or_else(|| color_eyre::eyre::eyre!("Profile device missing path"))?;
			Some(DeviceProfile::load_from_file(path)?)
		} else {
			let path = device_info
				.path
				.as_ref()
				.ok_or_else(|| color_eyre::eyre::eyre!("Physical device missing path"))?;
			eprintln!("DEBUG: ManagedDevice scanning capabilities for {}", path.display());
			let device = Device::new_from_path(path).with_context(|| format!("failed to create Device from {}", path.display()))?;
			let profile = DeviceProfile::from_device(&device)?;
			Some(profile)
		};

		let axis_configs = Self::convert_axis_configs(&device_config.axes);

		Ok(Self {
			device_config,
			device_info,
			cached_capabilities,
			virtual_output: None,
			axis_configs,
			running: Arc::new(AtomicBool::new(false)),
			clone_physical,
		})
	}

	/// Get a handle to stop this device
	pub fn stop_handle(&self) -> Arc<AtomicBool> {
		Arc::clone(&self.running)
	}

	/// Run the device (blocking) - handles virtual device creation, device connection, and event processing
	pub fn run(&mut self) -> Result<()> {
		self.running.store(true, Ordering::SeqCst);
		let mut current_input_device: Option<Device> = None;

		if self.clone_physical {
			println!("Waiting for physical device to connect for cloning...");
			while current_input_device.is_none() && self.running.load(Ordering::SeqCst) {
				current_input_device = self.try_connect_for_runtime();
				if current_input_device.is_none() {
					thread::sleep(Duration::from_secs(1));
				}
			}

			if let Some(ref input_device) = current_input_device {
				// Create virtual device by cloning the physical device
				let output = UInputDevice::create_from_device(input_device).context("creating UInputDevice from connected physical device")?;
				println!("Virtual device cloned from physical device:");
				let device_path = output.devnode().unwrap();
				let device_for_reading =
					Device::new_from_path(device_path).context("creating Device from cloned UInputDevice for diagnostics")?;
				print_device_info(&device_for_reading);
				self.virtual_output = Some(output);
			}
		} else {
			let virtual_output = self.create_virtual_output()?;
			self.virtual_output = Some(virtual_output);
		}

		while self.running.load(Ordering::SeqCst) {
			if let Some(ref mut input_device) = current_input_device {
				// Try to read events from physical device
				match input_device.next_event(ReadFlag::NORMAL | ReadFlag::BLOCKING) {
					Ok((status, event)) => match status {
						ReadStatus::Success => {
							if let Some(modified_event) = self.process_event(event) {
								eprintln!("DEBUG: Modified event: {modified_event:?}");
								if let Some(ref output) = self.virtual_output {
									if let Err(e) = output.write_event(&modified_event) {
										eprintln!("DEBUG: Error writing event to virtual device: {e}");
									}
								}
							}
						}
						ReadStatus::Sync => {} // sync handled via normal EV_SYN(SYN_REPORT) events
					},
					Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
						// FIXME: our syscall is supposed to block
						thread::sleep(Duration::from_millis(1));
						continue;
					}
					Err(e) => {
						eprintln!("DEBUG: Device {} errored, {e}, will attempt reconnection", self.device_config.name);
						let _ = input_device.grab(GrabMode::Ungrab);
						current_input_device = None;
					}
				}
			} else {
				current_input_device = self.try_connect_for_runtime();
				if current_input_device.is_some() {
					eprintln!("Device {} connected successfully", self.device_config.name);
				} else {
					thread::sleep(Duration::from_secs(1));
				}
			}
			thread::sleep(Duration::from_micros(100));
		}

		if let Some(ref mut input_device) = current_input_device {
			let _ = input_device.grab(GrabMode::Ungrab);
		}

		Ok(())
	}

	/// Create virtual output device using cached capabilities (no device re-opening)
	fn create_virtual_output(&self) -> Result<UInputDevice> {
		let default_config = OutputDeviceConfig {
			name: format!("Curved {}", self.device_info.name),
			vendor_id: None,
			product_id: None,
			version: None,
			bus_type: None,
		};
		let output_config = self.device_config.output_device.as_ref().unwrap_or(&default_config);

		eprintln!("DEBUG: Creating virtual device '{}'", output_config.name);

		if let Some(ref profile) = self.cached_capabilities {
			match create_virtual_device_from_profile(profile, output_config) {
				Ok(virtual_device) => {
					eprintln!("DEBUG: Successfully created virtual device '{}'", output_config.name);
					Ok(virtual_device)
				}
				Err(e) => {
					eprintln!("DEBUG: Failed to create virtual device '{}': {}", output_config.name, e);
					Err(e)
				}
			}
		} else {
			let err = color_eyre::eyre::eyre!("No cached capabilities available for virtual device creation");
			eprintln!("DEBUG: {err}");
			Err(err)
		}
	}

	/// Try to connect to the physical device for runtime - always uses VID/PID/Version matching
	fn try_connect_for_runtime(&self) -> Option<Device> {
		thread::sleep(Duration::from_millis(500));

		let target_vid = self.device_info.vendor_id;
		let target_pid = self.device_info.product_id;
		let target_version = self.device_info.version;

		eprintln!("DEBUG: try_connect_for_runtime searching for VID={target_vid:04x}/PID={target_pid:04x}/Version={target_version:04x}");

		match DeviceInfo::obtain_device_list() {
			Ok(devices) => {
				for device_info in devices {
					if device_info.vendor_id == target_vid && device_info.product_id == target_pid && device_info.version == target_version {
						if let Some(ref path) = device_info.path {
							eprintln!(
								"DEBUG: try_connect_for_runtime found matching device {}, attempting connection",
								path.display()
							);

							match Device::new_from_path(path) {
								Ok(mut input_device) => {
									let fd = input_device.file().as_raw_fd();
									unsafe {
										let flags = libc::fcntl(fd, libc::F_GETFL);
										if flags != -1 {
											let _ = libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
										}
									}

									eprintln!("DEBUG: try_connect_for_runtime opened {}, attempting grab", path.display());
									if input_device.grab(GrabMode::Grab).is_ok() {
										eprintln!("DEBUG: try_connect_for_runtime successfully grabbed {}", path.display());
										println!("Physical device connected:");
										print_device_info(&input_device);
										let _ = setup_device_permissions(path);
										return Some(input_device);
									} else {
										eprintln!("DEBUG: try_connect_for_runtime failed to grab {}", path.display());
									}
								}
								Err(e) => {
									eprintln!("DEBUG: try_connect_for_runtime failed to open {}: {}", path.display(), e);
								}
							}
						}
					}
				}
			}
			Err(e) => {
				eprintln!("DEBUG: try_connect_for_runtime failed to obtain device list: {e}");
			}
		}

		eprintln!("DEBUG: try_connect_for_runtime failed to find matching device");
		None
	}

	fn process_event(&self, event: InputEvent) -> Option<InputEvent> {
		match event.event_type() {
			Some(EventType::EV_ABS) => {
				let code = event.event_code;
				let axis_code = match code {
					EventCode::EV_ABS(EV_ABS::ABS_X) => 0,
					EventCode::EV_ABS(EV_ABS::ABS_Y) => 1,
					EventCode::EV_ABS(EV_ABS::ABS_RZ) => 5,
					_ => return Some(event),
				};

				let modified_value = self
					.axis_configs
					.get(&axis_code)
					.map(|config| self.apply_axis_curve(event.value, config))
					.unwrap_or(event.value);

				eprintln!("Absolute event: {event:?} -> {modified_value:?}");
				Some(InputEvent::new(&event.time, &code, modified_value))
			}
			Some(EventType::EV_SYN | EventType::EV_FF | EventType::EV_FF_STATUS) => Some(event),
			None => None,
			Some(_) => Some(event),
		}
	}

	fn apply_axis_curve(&self, value: i32, config: &AxisConfig) -> i32 {
		match &config.curve {
			Some(CurveType::Polynomial { power, deadzone }) => self.apply_polynomial_curve(value, *power, *deadzone),
			Some(CurveType::Nurbs(_nurbs_config)) => {
				eprintln!("NURBS curves not yet implemented, using polynomial fallback");
				self.apply_polynomial_curve(value, 2.0, 0.01)
			}
			None => value,
		}
	}

	/// Apply polynomial curve: output = sign(input) * |input|^power
	fn apply_polynomial_curve(&self, value: i32, power: f64, deadzone: f64) -> i32 {
		let normalized = (value as f64 - 32767.5) / 32767.5;
		if normalized.abs() < deadzone {
			return 32767;
		}
		let curved = normalized.abs().powf(power) * normalized.signum();
		((curved * 32767.5 + 32767.5) as i32).clamp(0, 65535)
	}

	fn convert_axis_configs(axes: &HashMap<String, AxisConfig>) -> HashMap<u16, AxisConfig> {
		let mut result = HashMap::new();
		for (axis_name, config) in axes {
			let axis_code = match axis_name.as_str() {
				"ABS_X" => 0,
				"ABS_Y" => 1,
				"ABS_Z" => 2,
				"ABS_RX" => 3,
				"ABS_RY" => 4,
				"ABS_RZ" => 5,
				_ => {
					eprintln!("Unknown axis name: {axis_name}");
					continue;
				}
			};
			result.insert(axis_code, config.clone());
		}

		result
	}

	fn find_device_internal(selector: &DeviceSelector) -> Result<DeviceInfo> {
		match selector {
			DeviceSelector::Name(name) => DeviceInfo::with_name(name, None, None),
			DeviceSelector::NameAndPhys { name, phys } => DeviceInfo::with_name(name, Some(phys), None),
			DeviceSelector::VidPidVersion { vid, pid, version } => DeviceInfo::with_name("", None, Some((*vid, *pid, *version))),
			DeviceSelector::NameWithIds { name, vid, pid, version } => DeviceInfo::with_name(name, None, Some((*vid, *pid, *version))),
		}
	}
}

#[derive(Default)]
pub struct DeviceManager {
	managed_devices: Vec<ManagedDevice>,
	stop_handles: Vec<Arc<AtomicBool>>,
	thread_handles: Vec<thread::JoinHandle<Result<()>>>,
}

impl DeviceManager {
	pub fn add_device(&mut self, device_config: DeviceConfig, clone_physical: bool) -> Result<()> {
		let managed_device = ManagedDevice::new(device_config, clone_physical)?;
		let stop_handle = managed_device.stop_handle();

		self.managed_devices.push(managed_device);
		self.stop_handles.push(stop_handle);

		Ok(())
	}

	pub fn start_all(&mut self) -> Result<()> {
		self.thread_handles.clear();
		for mut device in self.managed_devices.drain(..) {
			let device_name = device.device_config.name.clone();
			println!("Starting device: {}", device_name);

			let thread_handle = thread::spawn(move || device.run());

			self.thread_handles.push(thread_handle);
		}

		println!("All {} devices started", self.thread_handles.len());
		Ok(())
	}

	pub fn stop_all(&mut self) -> Result<()> {
		println!("Stopping all devices...");
		for stop_handle in &self.stop_handles {
			stop_handle.store(false, Ordering::SeqCst);
		}
		for thread_handle in self.thread_handles.drain(..) {
			thread_handle.join().unwrap_or_else(|_| {
				eprintln!("Failed to join device thread");
				Ok(())
			})?;
		}

		println!("All devices stopped");
		Ok(())
	}

	pub fn device_count(&self) -> usize {
		self.managed_devices.len() + self.thread_handles.len()
	}
}

fn main() -> Result<()> {
	color_eyre::install()?;

	// Check for command line flags
	let args: Vec<String> = std::env::args().collect();
	let show_devices = args.contains(&"--list-devices".to_string()) || args.contains(&"--show-devices".to_string());
	let save_profile = args.contains(&"--save-profile".to_string());
	let clone_physical = args.contains(&"--clone-physical".to_string());
	let rgb_demo = args.contains(&"--rgb-demo".to_string());

	if rgb_demo {
		return rgb::demo::run_demo();
	}

	if show_devices {
		println!("Available input devices:");
		let devices = DeviceInfo::obtain_device_list()?;
		for device in devices {
			println!("  - {device}");
			if !device.phys.is_empty() {
				println!("    Physical: {}", device.phys);
			}
			if let Some(ref path) = device.path {
				println!("    Path: {}", path.display());
			}
			println!();
		}
		return Ok(());
	}

	if save_profile {
		return save_all_profiles();
	}

	let config_path = "config.toml";
	let config = if std::path::Path::new(config_path).exists() {
		println!("Loading configuration from {config_path}");
		Config::load_from_file(config_path)?
	} else {
		eprintln!("Warning: {config_path} not found. Create one from the sample configuration.");
		eprintln!("Available devices:");
		let devices = DeviceInfo::obtain_device_list()?;
		for device in devices {
			eprintln!("  - {device}");
		}
		bail!("Configuration file is required");
	};

	let enabled_devices: Vec<_> = config.devices.into_iter().filter(|d| d.enabled).collect();

	println!("Found {} enabled device(s) in configuration", enabled_devices.len());

	if enabled_devices.is_empty() {
		println!("No devices are enabled in the configuration.");
		return Ok(());
	}

	let mut device_manager = DeviceManager::default();

	for device_config in enabled_devices {
		device_manager.add_device(device_config, clone_physical)?;
	}

	device_manager.start_all()?;

	println!("All devices started. Press Enter to stop...");

	let mut buffer = [0; 1];
	std::io::stdin().read_exact(&mut buffer)?;

	device_manager.stop_all()?;

	Ok(())
}
