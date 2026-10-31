---
title: Troubleshooting
order: 6
since: 0.2.0
---

# Troubleshooting

## The activation shortcut does not start a capture

Open **Settings → Shortcut** and check that the global shortcut is enabled and registered. Choose another combination if another app owns it.

Hyprland adds the configured binding while Lens is running. Other Wayland desktops may not let an app register global shortcuts; add **arcade-lens --capture** in the desktop’s keyboard settings. On Hyprland with permission enforcement, allow the screenshot portal tools described in [the platform notes](https://github.com/qa-p1/Arcade-lens/blob/main/docs/STATUS.md).

## OCR returns no text

On Linux, install Tesseract with English language data or download it from **Settings → General**. On Windows and macOS Lens uses system OCR, but those app builds have not been run interactively on a real desktop.

Small interface text can be difficult to read. Select a larger area or move closer to the screen text before capturing. Lens upscales small selections, but OCR cannot recover characters the engine did not detect.

## The selected screen does not match what I see now

Lens works from a frozen screenshot captured before the selection window appears. This lets you select menus and tooltips that would disappear if the screen changed. Start a new capture to use a fresh image.

On Wayland, the portal screenshot itself is the frozen image. Lens cannot list or select windows there, and live pins or recording are not available.

## A pin or window action is missing

Some actions depend on the platform. Wayland does not support Lens window commands, live pins, or window recording. macOS window commands and the system share sheet are not implemented.

## A command or send action is unavailable

Lens may have detected a secret in the selection and hidden outbound actions. Review the selected content and Privacy settings. If you choose an action that sends data or runs a command, inspect the preview and confirm it.

## I am on Windows or macOS

Both platforms build and pass tests in CI, but neither app has been run interactively on a real desktop. macOS also requires Screen Recording permission for capture and the app is not notarized; right-click it and choose **Open** on first launch.
