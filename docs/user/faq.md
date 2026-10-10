---
title: FAQ
order: 5
since: 0.1.1
---

# Frequently asked questions

## Does Lens upload my screen or use cloud AI?

No. Built-in recognition runs on your computer. Actions that send data to a website or another app are marked and preview their payload. Reverse image search is optional and uploads pixels if you configure it.

## Why did Lens hide an online action?

With **Hide online actions for selections that contain passwords, keys or tokens** enabled, Lens removes outbound actions when its recognizers find something that looks like a secret. The guard also checks again when an action runs.

## Do I need Tesseract?

Linux OCR uses Tesseract. Install it with English language data, or choose **Settings → General → Download Tesseract**. The downloaded copy is private to your user and shared by Arcade apps. Windows and macOS use system OCR, but those builds have not been run interactively on a real desktop.

## Can I select a window on Wayland?

Wayland does not expose a window list to Lens. Use the screenshot portal to select a region from its frozen capture. Window detection and commands are available on X11 and Windows; macOS currently supports window detection only.

## Does Lens keep what I selected?

No history is kept by default. If you enable history in Settings, Lens stores selection thumbnails and recognized content locally. Usage learning stores only which action was chosen for a result type.

## Can Lens run commands that it reads from the screen?

A command from the screen is never run without confirmation. Dangerous commands need Ctrl+Enter, Cmd+Enter on macOS, or a click. **Open Terminal + Paste** fills an editable command prompt and runs nothing.
