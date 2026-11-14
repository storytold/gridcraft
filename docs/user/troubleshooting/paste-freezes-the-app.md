---
title: Clicking Paste freezes the app
description: In GridCraft 0.5.0, clicking the Paste button or its arrow in the ribbon freezes the app. GridCraft 0.6.1 fixes it.
order: 20
verified: 0.6.1
---

In GridCraft 0.5.0, clicking **Paste** or its drop-down arrow on the **Home** tab freezes the app. GridCraft 0.6.1 fixes this, so update to 0.6.1 or newer.

## Symptoms

- You click **Paste**, or the small arrow next to it, and the window stops responding.
- On Linux, one reporter saw the process end a few seconds later with `SIGABRT`.
- It happens on macOS and Linux. Reports do not say whether it happens on Windows.

## Fix

Download the [latest release](https://github.com/storytold/gridcraft/releases/latest) and [install it](../getting-started/install.md).

If you must stay on 0.5.0, save your work before you click **Paste** (see [Save a workbook or export a copy](../guides/save-and-export.md)). The reports describe the ribbon button and its arrow only.

## Why this happens

A maintainer traced it to the ribbon in 0.5.0: when you click **Paste**, the ribbon checks whether you right-clicked while it holds a lock that the app's window library also needs, so the app waits for itself. The clipboard is not involved. The fix ([issue #273](https://github.com/storytold/gridcraft/issues/273), fix #272) is in 0.6.1.

## Still stuck?

If 0.6.1 still freezes when you click **Paste**, comment on [issue #273](https://github.com/storytold/gridcraft/issues/273) or ask in [Discord](https://discord.gg/artcraft). Say which system you use.
