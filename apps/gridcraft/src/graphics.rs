//! Graphics instance configuration, applied before wgpu can initialize any driver: Windows
//! backends and the DirectX 12 shader compiler, and the Intel Mac indirect-validation skip.
//!
//! Vulkan is never initialized unless `WGPU_BACKEND` asks for it: enumerating Vulkan adapters
//! crashes inside Intel's `igvk64.dll` on some UHD Graphics drivers (#120), and that access
//! violation happens in the driver, where no Rust error handling can catch it.
//!
//! DirectX 12 shaders are compiled with FXC (`d3dcompiler_47.dll`, part of Windows), never with
//! whichever `dxcompiler.dll` the DLL search path happens to find (#339, #354): see
//! [`dx12_compiler`].

// On macOS only the indirect-validation items are used outside tests; on Windows the other way
// round, so those two carry their own `allow`.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use eframe::{
    NativeOptions,
    egui_wgpu::WgpuSetup,
    wgpu::{Backends, DeviceType, Dx12Compiler, InstanceFlags},
};

/// wgpu compiles its indirect-call validation compute shaders inside `request_device`. On Intel
/// Macs (Iris 5100 in a Mac mini 2014) Metal's compiler aborts on them ("Compiler encountered an
/// internal error"), the device is lost and eframe exits before the window opens. GridCraft issues
/// no indirect draws or dispatches, so Intel Macs skip them; every other GPU keeps wgpu's default.
/// Same as WordCraft (storytold/wordcraft#2652) and PhotoCraft (storytold/PhotoCraft#3171).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn skips_indirect_validation(macos: bool, intel: bool) -> bool {
    macos && intel
}

/// Leaves wgpu's indirect-call validation out of the instance eframe creates when `skip`.
/// `WGPU_VALIDATION_INDIRECT_CALL=1` still turns it back on.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn configure_indirect_validation(options: &mut NativeOptions, skip: bool) {
    if !skip {
        return;
    }
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.flags = create.instance_descriptor.flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL).with_env();
    }
}

/// What the start-up probe learned about one DirectX 12 adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dx12Adapter {
    /// `Cpu` is WARP (Microsoft's software rasteriser): it works, but every frame is drawn on the
    /// processor, which makes the app lag on older machines.
    pub device_type: DeviceType,
    /// Whether the adapter offers the limits eframe will ask for when it creates the device.
    pub meets_limits: bool,
}

/// The backends to initialize on Windows.
///
/// An explicit `backend_override` (normally `Backends::from_env()`, i.e. `WGPU_BACKEND`) always
/// wins. Otherwise DirectX 12 only, as long as `dx12_adapters` (what a DirectX 12-only probe found)
/// contains a usable hardware GPU. Without one, OpenGL is the way to the GPU: graphics chips whose
/// driver has no DirectX 12 support still have an OpenGL driver, and GL can't reach the AMD driver
/// crash described in [`configure_backends`], because an AMD GPU with a driver shows up as a
/// DirectX 12 adapter and keeps us on DirectX 12. When DirectX 12 offers only WARP, it stays
/// enabled next to GL as a last resort for machines without an OpenGL driver either (virtual
/// machines): wgpu prefers any GPU over a software adapter, so it's only picked when GL has
/// nothing better. Vulkan is never part of the default.
pub fn choose_backends(backend_override: Option<Backends>, dx12_adapters: &[Dx12Adapter]) -> Backends {
    if let Some(backends) = backend_override {
        return backends;
    }
    let hardware = |a: &&Dx12Adapter| a.device_type != DeviceType::Cpu;
    if dx12_adapters.iter().filter(hardware).any(|a| a.meets_limits) {
        Backends::DX12
    } else if !dx12_adapters.is_empty() && !dx12_adapters.iter().any(|a| hardware(&a)) {
        Backends::DX12 | Backends::GL
    } else {
        // No DirectX 12 adapter at all, or only GPUs that can't run the app: GL alone, so a
        // DirectX 12 GPU that would fail device creation isn't picked over the OpenGL one.
        Backends::GL
    }
}

/// The backends for a second start when the first one failed with a graphics error before the
/// app was created (no adapter, no device, no surface): the other one of DirectX 12 and OpenGL.
/// `None` when `WGPU_BACKEND` chose the backends (the user asked for exactly those) or the first
/// start already tried both. Never Vulkan (#120).
pub fn fallback_backends(backend_override: Option<Backends>, first: Backends) -> Option<Backends> {
    if backend_override.is_some() {
        return None;
    }
    let rest = (Backends::DX12 | Backends::GL).difference(first);
    (!rest.is_empty()).then_some(rest)
}

/// The DirectX 12 shader compiler: `env_choice` (normally `Dx12Compiler::from_env()`, i.e.
/// `WGPU_DX12_COMPILER`, for troubleshooting) or else FXC.
///
/// wgpu's default (`Auto`) loads the first `dxcompiler.dll` on the DLL search path. GridCraft ships
/// none, so that is another program's copy (a Windows SDK, an Edge or WebView folder, anything on
/// `PATH`). Without a matching `dxil.dll` beside it, DXC prints "DXIL signing library not found",
/// wgpu treats that warning as a compile error, its internal compute pipelines fail, the device is
/// lost and the app quits before the first frame ("Parent device is lost"). FXC is part of Windows
/// and needs no extra DLL; egui's shaders don't need anything newer.
pub fn dx12_compiler(env_choice: Option<Dx12Compiler>) -> Dx12Compiler {
    env_choice.unwrap_or(Dx12Compiler::Fxc)
}

/// Restricts the wgpu instance eframe creates to `backends`, and pins its DirectX 12 shader
/// compiler ([`dx12_compiler`]).
pub fn configure_backends(options: &mut NativeOptions, backends: Backends) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backend_options.dx12.shader_compiler = dx12_compiler(Dx12Compiler::from_env());
        // eframe's default includes Vulkan and GL. Creating the GL backend can crash inside AMD's
        // atio6axx.dll, and enumerating Vulkan adapters inside Intel's igvk64.dll (#120), before
        // adapter selection or any Rust error handling runs, so the window never appears.
        // Choosing an adapter afterwards is too late: only leaving the backend out of the
        // instance avoids it. This matches the verified WGPU_BACKEND=dx12 workaround without
        // mutating process environment variables.
        create.instance_descriptor.backends = backends;
    }
}

/// Chooses the backends ([`choose_backends`]), applies them to `options` and returns them.
/// Without a `backend_override` this first probes DirectX 12 with a short-lived instance of its
/// own, which is dropped before eframe creates the real one.
#[cfg(target_os = "windows")]
pub fn configure(options: &mut NativeOptions, backend_override: Option<Backends>) -> Backends {
    let backends = match backend_override {
        Some(backends) => {
            log::info!("graphics: WGPU_BACKEND override, backends {backends:?}");
            backends
        }
        None => {
            let adapters = probe_dx12(options);
            let backends = choose_backends(None, &adapters);
            log::info!("graphics: {} DirectX 12 adapter(s) {adapters:?}, backends {backends:?}", adapters.len());
            backends
        }
    };
    configure_backends(options, backends);
    backends
}

/// Lists the DirectX 12 adapters, checking each against the limits eframe's device descriptor
/// will require. Empty when this build has no DirectX 12 backend.
#[cfg(target_os = "windows")]
fn probe_dx12(options: &NativeOptions) -> Vec<Dx12Adapter> {
    use eframe::wgpu::{Instance, InstanceDescriptor};
    // `Instance::new` panics when the build has no backend for the platform.
    if !Instance::enabled_backend_features().contains(Backends::DX12) {
        return Vec::new();
    }
    let WgpuSetup::CreateNew(create) = &options.wgpu_options.wgpu_setup else {
        return Vec::new();
    };
    let instance = Instance::new(InstanceDescriptor { backends: Backends::DX12, ..InstanceDescriptor::new_without_display_handle() });
    pollster::block_on(instance.enumerate_adapters(Backends::DX12))
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            let required = (create.device_descriptor)(adapter).required_limits;
            let meets_limits = required.check_limits(&adapter.limits());
            log::info!("graphics: DirectX 12 adapter {:?} ({:?}, driver {:?}), limits ok: {meets_limits}", info.name, info.device_type, info.driver);
            Dx12Adapter { device_type: info.device_type, meets_limits }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_macs_skip_the_indirect_validation_shaders() {
        // Metal's Intel compiler aborted on them, losing the device before the window opened.
        assert!(skips_indirect_validation(true, true));
        // Apple silicon, Windows and Linux keep wgpu's validation.
        for (macos, intel) in [(true, false), (false, true), (false, false)] {
            assert!(!skips_indirect_validation(macos, intel), "macos={macos} intel={intel}");
        }
        let flags = |options: &NativeOptions| {
            let WgpuSetup::CreateNew(create) = &options.wgpu_options.wgpu_setup else {
                panic!("default native options must create a wgpu instance");
            };
            create.instance_descriptor.flags
        };
        let mut options = NativeOptions::default();
        let before = flags(&options);
        configure_indirect_validation(&mut options, false);
        assert_eq!(flags(&options), before, "flags untouched when not skipping");
        if std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_none() {
            configure_indirect_validation(&mut options, true);
            assert!(!flags(&options).contains(InstanceFlags::VALIDATION_INDIRECT_CALL));
        }
    }

    fn configured_backends(backends: Backends) -> Backends {
        let mut options = NativeOptions::default();
        configure_backends(&mut options, backends);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.backends
    }

    const fn adapter(device_type: DeviceType, meets_limits: bool) -> Dx12Adapter {
        Dx12Adapter { device_type, meets_limits }
    }

    #[test]
    fn windows_default_never_initializes_vulkan() {
        // eframe's own default (PRIMARY | GL) would initialize Vulkan and OpenGL as well.
        let WgpuSetup::CreateNew(eframe_default) = NativeOptions::default().wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        assert!(eframe_default.instance_descriptor.backends.contains(Backends::GL));
        let gpu = [adapter(DeviceType::IntegratedGpu, true)];
        assert_eq!(configured_backends(choose_backends(None, &gpu)), Backends::DX12);
        // #120: whatever the DirectX 12 probe finds, and whatever the retry picks, no Vulkan.
        let probes: [&[Dx12Adapter]; 4] = [&[], &gpu, &[adapter(DeviceType::Cpu, true)], &[adapter(DeviceType::IntegratedGpu, false)]];
        for adapters in probes {
            let first = choose_backends(None, adapters);
            assert!(!first.contains(Backends::VULKAN), "{adapters:?}");
            if let Some(second) = fallback_backends(None, first) {
                assert!(!second.contains(Backends::VULKAN), "{adapters:?}");
            }
        }
    }

    #[test]
    fn explicit_backend_override_is_preserved() {
        let warp_only = [adapter(DeviceType::Cpu, true)];
        for backends in [Backends::VULKAN, Backends::GL, Backends::DX12, Backends::VULKAN | Backends::DX12, Backends::empty()] {
            // Whatever the probe would have found, the override wins (the probe doesn't run then),
            // and a failed start isn't retried with something the user didn't ask for.
            assert_eq!(choose_backends(Some(backends), &[]), backends);
            assert_eq!(choose_backends(Some(backends), &warp_only), backends);
            assert_eq!(configured_backends(backends), backends);
            assert_eq!(fallback_backends(Some(backends), backends), None);
        }
    }

    #[test]
    fn gl_when_dx12_has_no_usable_gpu() {
        // No DirectX 12 driver, so DirectX 12 offers only WARP (or nothing) and lags.
        let warp_only = [adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &warp_only), Backends::GL | Backends::DX12);
        assert_eq!(choose_backends(None, &[]), Backends::GL);
        // A GPU that can't create eframe's device: GL alone, so it isn't picked over the GL one.
        let weak_gpu = [adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &weak_gpu), Backends::GL);
        // Any usable GPU keeps DirectX 12 only, next to WARP or a weaker GPU.
        let mixed = [adapter(DeviceType::Cpu, true), adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::DiscreteGpu, true)];
        assert_eq!(choose_backends(None, &mixed), Backends::DX12);
    }

    #[test]
    fn failed_start_retries_with_the_other_backend() {
        assert_eq!(fallback_backends(None, Backends::DX12), Some(Backends::GL));
        assert_eq!(fallback_backends(None, Backends::GL), Some(Backends::DX12));
        // Both were already tried: nothing left to fall back to.
        assert_eq!(fallback_backends(None, Backends::DX12 | Backends::GL), None);
    }

    #[test]
    fn dx12_compiles_shaders_with_fxc_unless_asked() {
        // #339, #354: wgpu's `Auto` would load a stray dxcompiler.dll and lose the device.
        assert!(matches!(dx12_compiler(None), Dx12Compiler::Fxc));
        assert!(matches!(dx12_compiler(Some(Dx12Compiler::Auto)), Dx12Compiler::Auto));
        assert!(matches!(dx12_compiler(Some(Dx12Compiler::default_dynamic_dxc())), Dx12Compiler::DynamicDxc { .. }));
        // eframe's own default is `Auto`; the configured instance (first start and retry) uses FXC.
        let WgpuSetup::CreateNew(eframe_default) = NativeOptions::default().wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        if Dx12Compiler::from_env().is_none() {
            assert!(matches!(eframe_default.instance_descriptor.backend_options.dx12.shader_compiler, Dx12Compiler::Auto));
            let mut options = NativeOptions::default();
            configure_backends(&mut options, Backends::DX12);
            let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
                panic!("default native options must create a wgpu instance");
            };
            assert!(matches!(create.instance_descriptor.backend_options.dx12.shader_compiler, Dx12Compiler::Fxc));
        }
    }
}
