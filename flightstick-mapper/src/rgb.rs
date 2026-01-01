// SPDX-FileCopyrightText: 2025 LunNova
// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use color_eyre::eyre::{Context, Result, bail};
use colorsys::{Hsl, Rgb};
use rusb::{Device, DeviceHandle, GlobalContext};
use std::{collections::HashMap, time::Duration};

pub mod demo;

const VID: u16 = 0x044f;
const INTERFACE: u8 = 1;
const ENDPOINT_OUT: u8 = 0x02;
const USB_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LedId(pub u8);

impl LedId {
	pub const THUMB: LedId = LedId(0x00);

	// Button LEDs
	pub const BUTTON_5: LedId = LedId(0x11);
	pub const BUTTON_6: LedId = LedId(0x10);
	pub const BUTTON_7: LedId = LedId(0x12);
	pub const BUTTON_8: LedId = LedId(0x13);
	pub const BUTTON_16: LedId = LedId(0x08);
	pub const BUTTON_17: LedId = LedId(0x07);
	pub const BUTTON_18: LedId = LedId(0x09);
	pub const BUTTON_19: LedId = LedId(0x0A);

	// Logo and decorative LEDs
	pub const TM_LOGO_1: LedId = LedId(0x01);
	pub const TM_LOGO_2: LedId = LedId(0x02);
	pub const TM_LOGO_3: LedId = LedId(0x03);
	pub const UPPER_CIRCLE_1: LedId = LedId(0x04);
	pub const UPPER_CIRCLE_2: LedId = LedId(0x05);
	pub const UPPER_CIRCLE_3: LedId = LedId(0x06);
	pub const UPPER_CIRCLE_4: LedId = LedId(0x0B);
	pub const UPPER_CIRCLE_5: LedId = LedId(0x0C);
	pub const UPPER_CIRCLE_6: LedId = LedId(0x0D);
	pub const UPPER_CIRCLE_7: LedId = LedId(0x0E);
	pub const UPPER_CIRCLE_8: LedId = LedId(0x0F);
}

/// Button number to LED mapping
pub const BUTTON_TO_LED: [(u8, LedId); 9] = [
	(0, LedId::THUMB),
	(5, LedId::BUTTON_5),
	(6, LedId::BUTTON_6),
	(7, LedId::BUTTON_7),
	(8, LedId::BUTTON_8),
	(16, LedId::BUTTON_16),
	(17, LedId::BUTTON_17),
	(18, LedId::BUTTON_18),
	(19, LedId::BUTTON_19),
];

/// Predefined LED groups
#[derive(Debug, Clone)]
pub struct LedGroups;

impl LedGroups {
	pub const TM_LOGO: &'static [LedId] = &[LedId::TM_LOGO_1, LedId::TM_LOGO_2, LedId::TM_LOGO_3];

	pub const UPPER_CIRCLES: &'static [LedId] = &[
		LedId::UPPER_CIRCLE_1,
		LedId::UPPER_CIRCLE_2,
		LedId::UPPER_CIRCLE_3,
		LedId::UPPER_CIRCLE_4,
		LedId::UPPER_CIRCLE_5,
		LedId::UPPER_CIRCLE_6,
		LedId::UPPER_CIRCLE_7,
		LedId::UPPER_CIRCLE_8,
	];

	pub const LEFT_BUTTONS: &'static [LedId] = &[LedId::BUTTON_5, LedId::BUTTON_6, LedId::BUTTON_7, LedId::BUTTON_8];

	pub const RIGHT_BUTTONS: &'static [LedId] = &[LedId::BUTTON_17, LedId::BUTTON_16, LedId::BUTTON_19, LedId::BUTTON_18];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceSide {
	Left,
	Right,
}

impl DeviceSide {
	pub fn pid(self) -> u16 {
		match self {
			DeviceSide::Left => 0x042a,
			DeviceSide::Right => 0x0422,
		}
	}

	pub fn from_pid(pid: u16) -> Option<Self> {
		match pid {
			0x042a => Some(DeviceSide::Left),
			0x0422 => Some(DeviceSide::Right),
			_ => None,
		}
	}
}

#[derive(Debug, Clone, Copy)]
pub struct RgbColor {
	pub r: u8,
	pub g: u8,
	pub b: u8,
}

impl RgbColor {
	pub const fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	pub fn from_hex(hex: &str) -> Result<Self> {
		let hex = hex.trim_start_matches('#');
		if hex.len() != 6 {
			bail!("Hex color must be 6 characters (e.g. 'FF0000' or '#FF0000')");
		}

		let r = u8::from_str_radix(&hex[0..2], 16).context("Invalid red component in hex color")?;
		let g = u8::from_str_radix(&hex[2..4], 16).context("Invalid green component in hex color")?;
		let b = u8::from_str_radix(&hex[4..6], 16).context("Invalid blue component in hex color")?;

		Ok(Self::new(r, g, b))
	}

	pub fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Self {
		let hsl = Hsl::new(hue, saturation, lightness, None);
		let rgb: Rgb = hsl.into();
		Self::new((rgb.red() * 255.0) as u8, (rgb.green() * 255.0) as u8, (rgb.blue() * 255.0) as u8)
	}

	pub fn scale(&self, factor: f64) -> Self {
		Self::new(
			((self.r as f64 * factor).clamp(0.0, 255.0)) as u8,
			((self.g as f64 * factor).clamp(0.0, 255.0)) as u8,
			((self.b as f64 * factor).clamp(0.0, 255.0)) as u8,
		)
	}

	pub fn as_bytes(&self) -> [u8; 3] {
		[self.r, self.g, self.b]
	}
}

pub struct ThrustmasterSolaris {
	device_handle: DeviceHandle<GlobalContext>,
	side: DeviceSide,
}

impl ThrustmasterSolaris {
	pub fn find_devices() -> Result<HashMap<DeviceSide, ThrustmasterSolaris>> {
		let mut devices = HashMap::new();

		for device in rusb::devices()?.iter() {
			let device_desc = device.device_descriptor()?;

			if device_desc.vendor_id() == VID {
				if let Some(side) = DeviceSide::from_pid(device_desc.product_id()) {
					match Self::open_device(device, side) {
						Ok(solaris) => {
							devices.insert(side, solaris);
						}
						Err(e) => {
							eprintln!("Failed to open {:?} device: {}", side, e);
						}
					}
				}
			}
		}

		Ok(devices)
	}

	pub fn open(side: DeviceSide) -> Result<Self> {
		for device in rusb::devices()?.iter() {
			let device_desc = device.device_descriptor()?;

			if device_desc.vendor_id() == VID && device_desc.product_id() == side.pid() {
				return Self::open_device(device, side);
			}
		}

		bail!("Thrustmaster Solaris {:?} device not found", side);
	}

	fn open_device(device: Device<GlobalContext>, side: DeviceSide) -> Result<Self> {
		let handle = device.open()?;

		// Detach kernel driver if active
		if handle.kernel_driver_active(INTERFACE)? {
			handle.detach_kernel_driver(INTERFACE).context("Failed to detach kernel driver")?;
		}

		handle.claim_interface(INTERFACE).context("Failed to claim USB interface")?;

		Ok(Self {
			device_handle: handle,
			side,
		})
	}

	pub fn side(&self) -> DeviceSide {
		self.side
	}

	pub fn send_led_colors(&mut self, led_colors: &HashMap<LedId, RgbColor>) -> Result<()> {
		let (thumbstick, others): (Vec<_>, Vec<_>) = led_colors.iter().partition(|(led, _)| **led == LedId::THUMB);

		for (&led_id, &color) in thumbstick {
			let mut packet = vec![0x01, 0x88, 0x81, 0xFF, led_id.0];
			packet.extend_from_slice(&color.as_bytes());
			self.send_packet(&packet)?;
			std::thread::sleep(Duration::from_millis(10));
		}

		for chunk in others.chunks(2) {
			let mut packet = vec![0x01, 0x08, 0x85, 0xFF];
			for (led_id, color) in chunk {
				packet.push(led_id.0);
				packet.extend_from_slice(&color.as_bytes());
			}
			self.send_packet(&packet)?;
			std::thread::sleep(Duration::from_millis(10));
		}

		Ok(())
	}

	fn send_packet(&mut self, packet: &[u8]) -> Result<()> {
		let bytes_written = self
			.device_handle
			.write_bulk(ENDPOINT_OUT, packet, USB_TIMEOUT)
			.context("Failed to write USB packet")?;

		if bytes_written != packet.len() {
			bail!("Incomplete USB packet write: {} of {} bytes", bytes_written, packet.len());
		}

		Ok(())
	}

	/// Warning: LED color changes may involve EEPROM writes with limited durability.
	pub fn set_single_led(&mut self, led_id: LedId, color: RgbColor) -> Result<()> {
		let mut colors = HashMap::new();
		colors.insert(led_id, color);
		self.send_led_colors(&colors)
	}

	/// Warning: LED color changes may involve EEPROM writes with limited durability.
	pub fn set_group(&mut self, leds: &[LedId], color: RgbColor) -> Result<()> {
		let colors: HashMap<_, _> = leds.iter().map(|&led| (led, color)).collect();
		self.send_led_colors(&colors)
	}

	/// Warning: LED color changes may involve EEPROM writes with limited durability.
	pub fn clear_all(&mut self) -> Result<()> {
		let black = RgbColor::new(0, 0, 0);
		let all_leds = [
			LedId::THUMB,
			LedId::BUTTON_5,
			LedId::BUTTON_6,
			LedId::BUTTON_7,
			LedId::BUTTON_8,
			LedId::BUTTON_16,
			LedId::BUTTON_17,
			LedId::BUTTON_18,
			LedId::BUTTON_19,
			LedId::TM_LOGO_1,
			LedId::TM_LOGO_2,
			LedId::TM_LOGO_3,
			LedId::UPPER_CIRCLE_1,
			LedId::UPPER_CIRCLE_2,
			LedId::UPPER_CIRCLE_3,
			LedId::UPPER_CIRCLE_4,
			LedId::UPPER_CIRCLE_5,
			LedId::UPPER_CIRCLE_6,
			LedId::UPPER_CIRCLE_7,
			LedId::UPPER_CIRCLE_8,
		];

		self.set_group(&all_leds, black)
	}
}

impl Drop for ThrustmasterSolaris {
	fn drop(&mut self) {
		let _ = self.device_handle.release_interface(INTERFACE);
		let _ = self.device_handle.attach_kernel_driver(INTERFACE);
	}
}

pub fn button_to_led(button: u8) -> Option<LedId> {
	BUTTON_TO_LED.iter().find_map(|&(btn, led)| (btn == button).then_some(led))
}

pub fn get_led_group(name: &str) -> Option<&'static [LedId]> {
	match name {
		"tm_logo" => Some(LedGroups::TM_LOGO),
		"upper_circles" => Some(LedGroups::UPPER_CIRCLES),
		"left_buttons" => Some(LedGroups::LEFT_BUTTONS),
		"right_buttons" => Some(LedGroups::RIGHT_BUTTONS),
		_ => None,
	}
}
