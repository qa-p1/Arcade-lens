---
title: Getting started
order: 2
since: 0.1.1
---

# Getting started

## Install Arcade Lens

Install Lens from **Arcade Tools → Apps** to install, update, or remove it. Tools can host Lens’s tray icon in one Arcade icon while it is running; Lens keeps its own icon otherwise.

### Linux

Download the AppImage and run it. On first launch, choose **Install** to add Lens to the app launcher, place it in **~/Applications/Arcade/**, add **arcade-lens** in **~/.local/bin**, and choose whether it starts at login. Choose **Just run it once** to use Lens without installing it. If **~/.local/bin** is not on PATH, the installer shows the exact line to add for your shell.

To uninstall, run **arcade-lens --uninstall**, use the launcher’s **Uninstall** action, or remove Lens in Arcade Tools. Your settings stay unless you choose to remove them.

Linux OCR needs Tesseract and English language data. Install it with your package manager, or use **Settings → General → Download Tesseract** to fetch a private copy shared by Arcade apps.

### Windows

Run the installer. It installs for your user without administrator access, adds a Start menu entry, offers a desktop shortcut and start-at-login options, and adds **arcade-lens** to PATH. If SmartScreen warns about the unsigned installer, choose **More info → Run anyway**. Uninstall from **Settings → Apps** or Arcade Tools.

Windows uses the OCR built into the system. The Windows app has not yet been run interactively on a real desktop.

### macOS

Open the dmg and drag Lens to Applications. If you run Lens from the dmg or Downloads, it offers to move itself to Applications. The app is not notarized, so right-click it and choose **Open** the first time.

macOS uses Apple Vision for OCR. The macOS app has not yet been run interactively on a real desktop.

## Make your first selection

Press **Ctrl+Alt+Shift+L** to capture the screen, then drag a rectangle around a URL, a short paragraph, or a QR code. Lens freezes the screen after capturing it, so menus and tooltips remain available to select. Press **Enter** to run the highlighted action, or press **Space** or **Tab** to browse more actions.

On Hyprland, Lens can bind this shortcut while it runs. On other Wayland desktops, add **arcade-lens --capture** as a keyboard shortcut in the desktop settings. Change the activation shortcut in **Settings → Shortcut**.

## Keep Lens available

Lens runs in the background so the activation shortcut and tray icon work. Click the tray icon for Settings; choose **Open Lens** to start a capture. From a terminal, **arcade-lens --capture** starts one immediately.
