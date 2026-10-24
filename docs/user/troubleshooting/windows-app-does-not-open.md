---
title: The app does not open on Windows
description: What to do when GridCraft flashes in Task Manager and quits, or never shows a window, on Windows 11.
order: 10
verified: 0.6.1
---

Update to GridCraft 0.6.1 first: it fixes the known causes of a window that flashes and closes. If 0.6.1 still does not open, start it with another graphics backend from PowerShell: `dx12`, or `gl` on Intel graphics.

## Symptoms

- You start GridCraft and no window appears.
- GridCraft shows up in Task Manager for a moment and then quits.
- The window flashes and GridCraft exits about a tenth of a second later, and `gridcraft.log` ends with `Parent device is lost`.
- Windows Event Viewer logs a crash in `igvk64.dll` (Intel's Vulkan driver) or `atio6axx.dll` (an AMD driver). The Intel crash logs in issues #59 and #120 show exception code `0xc0000005`.

## Fix

1. Download the [latest release](https://github.com/storytold/gridcraft/releases/latest) and [install it](../getting-started/install.md). GridCraft 0.6.1 fixes the `Parent device is lost` exit ([issue #339](https://github.com/storytold/gridcraft/issues/339)) and no longer loads the Vulkan driver that crashed on Intel graphics ([issue #120](https://github.com/storytold/gridcraft/issues/120)).
2. If 0.6.1 still does not open, open `gridcraft.log` (see **Still stuck?** below). Lines starting with `graphics:` say which graphics backend GridCraft chose.
3. Tell GridCraft which backend to use. Open PowerShell, go to the folder that contains `gridcraft.exe`, and run:

       $env:WGPU_BACKEND = "dx12"
       & .\gridcraft.exe
       Remove-Item Env:WGPU_BACKEND

   The last line restores the default for later launches. If `dx12` does not help, try `gl` in place of `dx12`.

On AMD graphics do not use `gl`: it can bring back the `atio6axx.dll` crash. On Intel graphics do not use `vulkan`: it can bring back the `igvk64.dll` crash.

## Why this happens

GridCraft 0.5.0 compiled its DirectX 12 shaders with DXC but did not ship `dxcompiler.dll` and `dxil.dll`, so on some PCs the graphics device was lost before the first frame. GridCraft 0.6.1 uses FXC, the compiler built into Windows, instead ([issue #339](https://github.com/storytold/gridcraft/issues/339)).

On some PCs a graphics driver crashes while the app is setting up graphics, before the window can open. In issue #59 the crash was in Intel's Vulkan driver, which crashed when asked to list the graphics adapters. For AMD, the README says letting the app also create an OpenGL instance can crash `atio6axx.dll`. That is why GridCraft starts with DirectX 12 only. In 0.6.1, if DirectX 12 finds no graphics card or fails to start, GridCraft switches to OpenGL by itself. Setting `WGPU_BACKEND` turns this off, and then GridCraft tries only the backend you chose.

## Still stuck?

Follow [Report a problem](get-help.md). GridCraft writes `gridcraft.log` to `%APPDATA%\GridCraft\logs`. Each start moves the previous log to `gridcraft.1.log` and that one to `gridcraft.2.log`, so the log of a run that crashed is still there after the next start. Attach these files when you [open an issue](https://github.com/storytold/gridcraft/issues), or ask in [Discord](https://discord.gg/artcraft). Say which Windows version, graphics card and driver version you have.
