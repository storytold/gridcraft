//! The device limits GridCraft asks the graphics adapter for, on every native platform.
//!
//! The window's surface is a texture, so its sides can't exceed the device's largest texture
//! side, and eframe asks for exactly 8192 whatever the GPU can do. A window across monitors that
//! add up to more (1080 + 3440 + 3840 = 8360 pixels side by side at 100 %), or one a mixed-DPI
//! drag scaled up (9048x5757 between 250 % and 100 % monitors), then failed `Surface::configure`,
//! which wgpu treats as fatal: the app closed. DirectX 12 and Metal hardware offer 16384.

use eframe::{NativeOptions, egui_wgpu::WgpuSetup, wgpu::Limits};
use std::sync::Arc;

/// `required` with the largest texture side the adapter's `supported` limits offer. Taking the
/// adapter's own value also never asks a smaller (downlevel) adapter for more than it has.
pub fn surface_texture_limits(mut required: Limits, supported: &Limits) -> Limits {
    required.max_texture_dimension_2d = supported.max_texture_dimension_2d;
    required
}

/// Makes the device eframe creates ask for the adapter's largest texture side
/// ([`surface_texture_limits`]), keeping eframe's other device requirements.
pub fn request_adapter_texture_size(options: &mut NativeOptions) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let device_descriptor = Arc::clone(&create.device_descriptor);
        create.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = device_descriptor(adapter);
            descriptor.required_limits = surface_texture_limits(descriptor.required_limits, &adapter.limits());
            descriptor
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_texture_size_is_the_adapters_own() {
        // Monitors adding up to 8360 pixels, or a mixed-DPI drag's 9048x5757, need more than
        // eframe's 8192; a downlevel adapter must not be asked for more than it has.
        for base in [Limits::default(), Limits::downlevel_webgl2_defaults()] {
            let eframe_request = Limits { max_texture_dimension_2d: 8192, ..base.clone() };
            for side in [4096, 8192, 16384, 32768] {
                let supported = Limits { max_texture_dimension_2d: side, ..base.clone() };
                let limits = surface_texture_limits(eframe_request.clone(), &supported);
                assert_eq!(limits.max_texture_dimension_2d, side);
                // Everything else eframe asks for is unchanged.
                assert_eq!(Limits { max_texture_dimension_2d: 8192, ..limits.clone() }, eframe_request);
                assert!(limits.check_limits(&supported));
            }
        }
    }

    #[test]
    fn native_options_keep_creating_their_own_device() {
        // The wrapper only replaces the device descriptor: eframe still creates the instance,
        // adapter and device, and graphics.rs can still restrict the backends afterwards.
        let mut options = NativeOptions::default();
        request_adapter_texture_size(&mut options);
        assert!(matches!(options.wgpu_options.wgpu_setup, WgpuSetup::CreateNew(_)));
    }
}
