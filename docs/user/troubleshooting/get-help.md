---
title: Report a problem
description: What to include and where to post when GridCraft misbehaves, including where to find the log file.
order: 40
verified: 0.6.1
---

Post in [Discord](https://discord.gg/artcraft) or open an issue in the [issue tracker](https://github.com/storytold/gridcraft/issues). Attach the log file.

## What to include

- Your system (Windows 11, macOS, Linux distribution) and the GridCraft version, for example 0.6.1.
- What you did, what you expected and what happened instead.
- The log file (see below). If the problem happens with one workbook, say what is special about it.

## Find the log file

GridCraft writes `gridcraft.log` in a `logs` folder next to its settings:

| System  | Folder                                                                                |
|---------|---------------------------------------------------------------------------------------|
| Windows | `%APPDATA%\GridCraft\logs\`                                                           |
| macOS   | `~/Library/Application Support/GridCraft/logs/`                                       |
| Linux   | `$XDG_CONFIG_HOME/gridcraft/logs/`, which is `~/.config/gridcraft/logs/` by default   |

Each start moves the previous log to `gridcraft.1.log`, and that one to `gridcraft.2.log`. If GridCraft crashed, the log of the crashed run is the `gridcraft.1.log` after your next start.
