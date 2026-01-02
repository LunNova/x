// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use super::ThrustmasterSolaris;
use color_eyre::eyre::Result;

/// Finds connected Thrustmaster Solaris devices and clears their LEDs.
pub fn run_demo() -> Result<()> {
	println!("Thrustmaster Solaris RGB Demo");
	println!("==============================");

	let devices = ThrustmasterSolaris::find_devices()?;

	if devices.is_empty() {
		println!("No Thrustmaster Solaris devices found.");
		println!("Make sure your device is connected and drivers are properly configured.");
		return Ok(());
	}

	for (side, mut device) in devices {
		println!("Found {:?} device, clearing LEDs...", side);
		device.clear_all()?;
	}

	println!("Demo complete!");
	Ok(())
}
