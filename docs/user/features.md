---
title: Features
order: 3
since: 0.2.0
---

# Features

## Select something on screen

Press the activation shortcut and draw a region, or click a highlighted window where window detection is supported. Lens captures all monitors and freezes that image before showing actions. Drag inside a selection to move it; drag its handles to resize it.

Use the arrow keys to move the selection. Hold Shift to resize it instead, and hold Ctrl for a larger step. On macOS, the larger-step modifier is Cmd. The image stays fixed while you work.

## Copy, save, pin, or annotate

These image actions are available as soon as Lens has a selection:

- **Copy** puts the selected region on the clipboard as an image.
- **Save** writes an image using the format selected in Settings.
- **Pin** keeps the captured region floating above other windows. Drag the pin to move it, scroll to zoom, and use Ctrl+scroll to change opacity. Right-click for copy, save, annotate, live refresh where supported, or close. Double-click or press Escape to close a pin.
- **Annotate** opens an editor with pen, highlighter, line, arrow, rectangle, ellipse, text, and pixelation tools. Pixelation can obscure private details. Undo and redo edits, then copy, save, or pin the result.
- **Measure** lets you draw a second rectangle to measure distance. Press M to enter or leave the mode, use arrow keys to place the cursor, and press C to copy the measured size.

## Read text and recognize content

Lens uses OCR to read text. Its built-in recognizers can identify:

- Links, email addresses, phone numbers, postal addresses, IPv4 and IPv6 addresses, and domain names.
- Dates and times, timecodes, timestamps, currencies, physical quantities, and coordinates. Ambiguous numeric dates keep both interpretations instead of silently choosing one.
- Shell commands, source code and language, error messages and stack traces, file paths, hashes, UUIDs, secrets and tokens.
- Tables in CSV, TSV, or Markdown form; document text for clean copy, PDF, or print; subtitles; and Git commits on forge pages.
- Colors, palettes, foreground and background colors, contrast, UI element measurements, QR codes, barcodes, images, icons, and media-player frames.

Actions depend on what Lens recognizes and which platform services are available. The action key is shown on each chip; those single-letter keys can be changed in **Settings → Actions**. Press **Enter** for the default action or **Space**/**Tab** for the full list and a filter.

## Use results safely

Recognition runs locally; Lens has no cloud inference or account. Selections are temporary unless you enable history. Usage learning stores only local counts of which action you chose for a kind of result.

Lens hides outbound actions when selected content appears to contain a password, key, or token. It applies this guard again when an action runs. An action that would send data or run a command shows what will happen. Commands read from the screen are not run without confirmation: dangerous commands require Ctrl+Enter (Cmd+Enter on macOS) or a click. **Open Terminal + Paste** places text in an editable prompt and does not run it.

## Customize Lens

Settings let you change the activation shortcut, OCR languages, screenshot format and folder, preferred or disabled actions, recognizers, action keys, saved chains, providers, plugins, connected apps, privacy, and history. You can disable selection history; it is off by default. The default online search providers are opened in your browser, and reverse image search is not configured by default because it uploads image pixels.

## Use chains and plugins

A chain combines compatible actions into a repeatable workflow. Lens checks the actions and their effects before running the chain.

Third-party recognizers and actions run out of process with declared permissions. Copy a plugin folder into the location opened from **Settings → Plugins → Open plugins folder**, then enable it. Review its permissions before using it.

## Platform behavior

- **Linux X11:** screen capture, native shortcut, window detection and commands, OCR with Tesseract, live pins, and FFmpeg-based window recording are implemented.
- **Linux Wayland:** capture uses the screenshot portal. There is no window list or window commands; live pins and recording are unavailable. Hyprland can bind the shortcut at runtime; elsewhere bind **arcade-lens --capture** yourself.
- **Windows:** capture uses xcap and OCR uses Windows.Media.Ocr. The build passes CI, but the app has not been run interactively on a real desktop.
- **macOS:** capture uses xcap and OCR uses Apple Vision. Screen Recording permission is needed; window detection is available but window commands are not implemented. The build passes CI, but the app has not been run interactively on a real desktop.

See [STATUS](https://github.com/qa-p1/Arcade-lens/blob/main/docs/STATUS.md) for the current verification record.
