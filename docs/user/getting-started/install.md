---
title: Install GridCraft
description: Download GridCraft for Windows 11, macOS or Linux from GitHub, or install it through ArtCraft Launcher.
order: 10
verified: 0.6.1
---

Download the file for your system from the [latest release](https://github.com/storytold/gridcraft/releases/latest) and install it. In the file names, `<version>` stands for the version number, for example `0.6.1`. The release also lists a `SHA256SUMS.txt` file with a checksum for every download.

## Windows 11

1. Open the [latest release](https://github.com/storytold/gridcraft/releases/latest).
2. Download `gridcraft-<version>-windows-x64.msi`. It fits most PCs (Intel and AMD, 64-bit).
3. Run the file and follow the installer. It offers an optional desktop shortcut, which is unchecked on a new installation.
4. Start GridCraft.

The installers and the app are code-signed.

For a Windows on Arm PC such as a Snapdragon laptop, download the `arm64` installer instead. The release also has a 32-bit `x86` installer.

To run GridCraft without installing it, download `gridcraft-<version>-windows-x64-portable.zip`, unzip it and run `gridcraft.exe`. The `arm64` and `x86` builds have a portable zip too.

If the app does not start, see [The app does not open on Windows](../troubleshooting/windows-app-does-not-open.md).

## macOS

1. Download `gridcraft-<version>-macos-universal.dmg`. It works on Apple silicon and Intel Macs. The app is signed and notarized.
2. Open it and drag GridCraft to Applications.
3. Open GridCraft from Applications.

The release also has `gridcraft-cli-<version>-macos-universal.zip`, a command-line tool for scripts. You do not need it to use the app.

## Linux

Pick one file from the release. Replace `x86_64` with `aarch64` on Arm.

| File                                        | Notes                                                    |
|---------------------------------------------|----------------------------------------------------------|
| `gridcraft-<version>-linux-x86_64.AppImage` | Runs anywhere. It updates itself with AppImageUpdate.    |
| `gridcraft-<version>-linux-x86_64.flatpak`  | Sandboxed. Install with `flatpak install --user <file>`. |
| `gridcraft-<version>-linux-x86_64.deb`      | For Debian and Ubuntu.                                   |
| `gridcraft-<version>-linux-x86_64.rpm`      | For Fedora, RHEL and openSUSE.                           |
| `gridcraft-<version>-linux-x86_64.tar.gz`   | Unpack anywhere.                                         |

The release also has RISC-V Linux builds (`gridcraft-<version>-linux-riscv64` as `.deb`, `.rpm` or `.tar.gz`) and a FreeBSD build (`gridcraft-<version>-freebsd-x86_64.tar.gz`).

## In a web browser

The release has `gridcraft-web-<version>.zip`, a static site that runs in a modern browser. Host it on any static web server. In the web version, **Open** uses the browser's file picker. Saving opens the browser's save window where the browser has one, and downloads the workbook in other browsers.

## Install with ArtCraft Launcher

ArtCraft Launcher lists GridCraft among the Craft apps, so you can install and update it from there. See [ArtCraft Launcher](https://getartcraft.com/docs/craft-launcher) for how.

## Next

- [Open a workbook](../guides/open-a-workbook.md)
- [Write formulas](../guides/write-formulas.md)
