# Arcade Lens architecture

Arcade Lens turns a screen selection into a set of *interpretations* and
exposes every useful action for them. This document describes how the code is
organized and why.

```
             ┌──────────────────────── platform shell ─────────────────────────┐
 shortcut ──▶│ capture (all monitors) → freeze → selection overlay → palette UI │
             └───────────────┬───────────────────────────────────▲──────────────┘
                             │ Selection {rect, pixels, windows} │ PaletteEntry[]
                             ▼                                   │
             ┌──────────── lens-core ────────────────────────────┴─────────────┐
             │ Engine ──▶ findings (streamed) ──▶ build_palette ──▶ invoke     │
             │   ▲                                   ▲      ▲          │       │
             │   │ Recognizers           Actions ────┘      │       Host trait  │
             │   │ (registry)            (registry)   UsageStore,  (clipboard, │
             │   │                                    Settings     open, …)    │
             └───┼──────────────────────────────────────────────────┼──────────┘
                 │                                                  │
        lens-recognizers                                   platform Host impl
        lens-actions (+ third-party plugins)
```

## Crates

| Crate | Responsibility |
|---|---|
| `lens-core` | Data model, capability graph, progressive engine, plugin registry, ranking, safety policy, chains, settings. No UI, no OS calls. |
| `lens-recognizers` | The 26 built-in recognizers (25 when no OCR engine is available): OCR integration (the platform engine, or the user's Tesseract), structured text, context (git commits, documents, subtitles), colors/palettes, UI inspection, image kind, windows, media frames, QR/barcodes. |
| `lens-actions` | The 156 built-in actions and the default chains. |
| `lens-platform` | OS services: monitor enumeration and capture, cursor, window list and window commands, keyboard focus, global shortcut (and Hyprland bindings), single-instance IPC, tray icon, autostart and launcher entries, native OCR engines. |
| `lens-plugins` | Out-of-process plugins: manifests, discovery, the JSON-lines protocol, and proxy recognizers/actions with permission enforcement. See [PLUGINS.md](PLUGINS.md). |
| `arcade-lens` | The application: the egui/eframe overlay, palette, pins, annotate, measure and settings windows; the desktop `Host`; configuration; the tray and desktop integration. |

## Process model

Lens runs as one long-lived background instance, started at login
(`arcade-lens --background`) or from the applications menu:

* A **tray icon** (StatusNotifierItem on Linux) shows that it is running. A
  click opens Settings; its menu is the one every Arcade app has: Open Lens
  (a capture), Open Settings, Restart Arcade Lens and Quit Arcade Lens.
  Restart starts a successor (`--restarting`) that waits for the old
  instance to exit. Start at login is a Settings switch.
* With **Connected apps** on, an Arcade Link server thread publishes Lens's
  manifest and serves its `lens.*` actions; `--arcade-invoke` answers a
  single request one-shot when Lens isn't running. See
  [ARCADE_LINK.md](ARCADE_LINK.md).
* On its first run outside a cargo `target` directory it turns on start at
  login, and at every start it keeps the login item and the
  applications-menu entry pointing at its executable.
* The **root window is the overlay**. While idle it is hidden (on X11 an
  override-redirect 1×1 window off screen), so an idle Lens draws nothing
  and does not repaint.
* On the shortcut (`global-hotkey`) or an IPC request, every monitor is
  captured **before** anything is shown. The overlay then displays the frozen
  image, so the capture never contains Lens itself. Other monitors get their
  own overlay viewports.
* Pins, annotate editors, settings and the recording indicator are separate
  viewports sharing `Arc<Mutex<AppState>>`. They outlive the overlay.
* Keyboard focus is requested natively (`lens_platform::focus_native`),
  since an override-redirect window gets no focus from the window manager.
* A second launch (`--capture`, `--settings`, `--restart`, `--quit`, or no
  option for Settings) connects to the running instance over loopback TCP. It
  authenticates with a random token stored in a 0600 file. With no instance
  running, it becomes the background instance.
* **Wayland** can't hide a window, so there the background instance has no
  window at all: each overlay, pin, editor and the settings window runs in
  its own short-lived process (`--window …`) whose root window is that
  window. On Hyprland, the activation shortcut and window rules are added to
  the compositor at runtime and re-added after a config reload.
* Recognition runs on the engine's thread pool. The UI polls the analysis
  stream each frame and only repaints while work is outstanding.
* On Linux (glibc) Lens caps malloc arenas at two before any thread starts.
  With rayon workers and a thread per Link connection, freed memory otherwise
  scattered across per-thread arenas and RSS crept under sustained OCR load
  (115 → 143 MiB over 600 requests); with two it holds at about 125 MiB.

## Core concepts

### Selection
A `Selection` is the captured pixels plus the rectangle in **physical pixels
in virtual-desktop space** (negative coordinates allowed), the monitor it is
on, and the windows visible at capture time. One canonical coordinate space is
what keeps the selection rectangle and the captured pixels identical on
mixed-DPI, multi-monitor setups; logical coordinates are derived only for
display.

### Capabilities and findings
A **capability** is an open-ended label (`url`, `qr-code`, `color`, or a
plugin's `com.example.isbn`). A **finding** is one interpretation of the
selection: a capability, a typed `Value`, a confidence, and provenance
(`derived_from`). A selection usually has many findings at once — a QR code
containing a URL yields `region`, `qr-code` and `url` — and none excludes the
others. The `CapabilityGraph` records *is-a* edges (`url` is `text`, `region`
is `image`) used for chain type checking.

Every selection starts with a `region` finding, so baseline actions (Copy,
Save, Pin, Annotate…) exist even if every recognizer fails.

### Recognizers form a dataflow graph
Recognizers declare which capabilities they *consume* and *produce*. The
engine offers each new finding to every recognizer consuming its capability:

```
region ──▶ OCR ──▶ text ──▶ url, email, path, command, code, error, table…
   │
   ├────▶ QR/barcode ──▶ qr-code ──▶ (same text recognizers) ──▶ url …
   ├────▶ color / palette / inspect / image-kind / window
```

There is no `if type == …` dispatch anywhere. Adding a recognizer means
registering it; nothing else changes.

### Engine: progressive, parallel, cancellable
* Cheap `Signals` (size, flatness, edge density, color count) are computed
  first and used by `should_run` gates — OCR is skipped on a flat color,
  palette extraction on a 3×3 pick.
* The `region` finding is emitted immediately; other results stream as
  `AnalysisEvent::Findings` the moment each recognizer finishes, cheapest
  scheduled first, on a shared thread pool (idle threads use no CPU).
* Findings are de-duplicated by `(capability, value)` so a URL found by OCR
  and inside a QR code appears once.
* Each job runs under `catch_unwind`; a failing recognizer produces a
  `RecognizerFailed` event and only loses its own results.
* `Analysis::cancel` (Escape, running an action, a new selection) stops
  scheduling and abandons outstanding work immediately; long recognizers
  poll the `CancelToken`.

### Actions are separate providers
An action declares: id, label, icon, group, accepted capabilities, what it
produces (for chaining), base priority, default key, **side effects**, and
required host features. The URL recognizer knows nothing about "Open",
"Generate QR" or "Send to Phone".

Optional hooks: `applies` (e.g. "the path exists"), `preview` (exactly what
will leave the machine), `confirmation` (dangerous actions), and `choices`
(when the user must pick an interpretation, e.g. an ambiguous date).

### Safety model
`Effects` bitflags classify every action:

| Class | Effects | Treatment |
|---|---|---|
| Pure | none | In-memory transform; result is copied when invoked from the palette. |
| Local | clipboard, files, launching apps, window control | Normal. |
| External | network, uploads content, sends to device | Marked ↗; preview shows the payload. |
| Dangerous | executes commands, deletes/overwrites, privileged | Always confirmed. |

Additional rules enforced in core, not left to individual actions:

* **Secret guard.** If a `secret` finding exists, any outbound action whose
  payload (its preview, or the finding's text) contains the secret is removed
  from the palette *and* refused by `invoke`. Pixel-only outbound actions on
  such a selection require confirmation.
* **OCR'd commands never run silently.** "Run in Terminal" is Dangerous and
  its confirmation lists the exact text and every detected risk (sudo,
  package management, deletion, pipe-to-shell, downloads, redirection…).
  "Open Terminal + Paste" pre-fills an editable prompt; nothing runs until
  the user presses Enter.
* **Ambiguity is surfaced.** `05/03/2024` produces both readings; calendar
  actions return `NeedsChoice` instead of guessing.
* **Plugin permissions.** A plugin manifest declares the maximum `Effects`
  its actions may have; actions exceeding them are rejected at registration,
  and ids must be namespaced under the plugin id. Out-of-process plugins
  also declare data permissions (`read-text`, `read-pixels`) that bound what
  Lens sends them, and they can only *request* effects, which Lens performs
  when the action declared them. See [PLUGINS.md](PLUGINS.md).

### Ranking
`build_palette` scores every (action, finding) pair:

```
score = action priority
      + 40 × specificity(capability) × confidence
           × (0.4 + 0.6 × coverage)                    # url ≫ text ≫ region

      + learned usage boost (≤ 25, decaying, local)
      + 40 if the user pinned the action as preferred
```

`coverage` is the share of its parent text a finding spans. A URL that *is*
the selection outranks one buried in a paragraph. The top N (default 5)
distinct actions become the primary row, with at most two per group and two
per finding, so the row isn't five "Copy" variants or five actions on one
email address; everything else is the
`•••` list. Keys are assigned per (action, finding), primary entries first,
honoring user overrides. `stabilize` keeps already-visible entries in place
while late results arrive. A newcomer replaces a slot only if it beats it by
a clear margin, and it takes that slot rather than reshuffling the row, so
the palette doesn't jump. `default_index` marks the Enter action.

### Usage learning
`UsageStore` keeps a decaying counter per (capability, action) — no content,
no account, resettable (Settings → Privacy). Half-life is 30 days and
the boost saturates so a few choices matter but hundreds don't drown out
relevance.

### Chains
A chain is `take <capability>` followed by action steps with parameters.
Validation walks the steps using each action's accepted/produced
capabilities and the is-a graph, so `take url → resize` is rejected at save
time. A chain's effects are the union of its steps'; chains with External or
Dangerous effects require confirmation. Chains appear in the palette when
the selection has a finding they can start from.

### Host
`Host` is the platform service boundary actions call: clipboard, open
URI/path (with modes: default, reveal, editor, terminal, Quick Look), terminal,
save (never overwrites), pin, annotate, share, send to device, window
commands, workspaces, download, local collections, measure mode, print. Unsupported services
are absent from `HostFeatures`, so actions needing them simply don't appear.

## Privacy

* Recognition is entirely local: OCR (the platform engine, or Tesseract),
  QR/barcodes (rxing), all text recognizers (deterministic rules).
* Selections are ephemeral; nothing is persisted unless an action does so.
* `Value`'s `Debug` never prints secrets or pixels; `Sensitive` wraps raw
  secret material.
* Usage learning stores only capability → action counters.
* External services are only contacted by actions that inherently need them
  (search, maps, WHOIS, currency rates via search), with configurable
  provider templates and a visible preview of what is sent.

## Platform layer

Shared logic stays in the crates above. `lens-platform` implements:

| Area | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Monitors & capture | RandR + `GetImage`, scale from `Xft.dpi` | `xdg-desktop-portal` Screenshot (ashpd) | xcap | xcap |
| Global shortcut | `global-hotkey` (XGrabKey) | Hyprland: runtime compositor binding; elsewhere bind `arcade-lens --capture` | `global-hotkey` (RegisterHotKey) | `global-hotkey` (Carbon) |
| Overlay | override-redirect viewports | portal image, fullscreen viewport | topmost viewports | topmost viewports |
| OCR | Tesseract | Tesseract | Windows.Media.Ocr | Vision |
| Window list | EWMH `_NET_CLIENT_LIST` | unavailable | xcap | xcap |
| Window commands | EWMH client messages | — | Win32 | — (needs Accessibility) |

Wayland deliberately prevents global capture and global hotkeys without
user-granted portals. There, the "freeze" is the portal screenshot itself,
and the activation shortcut is bound through the compositor.

Every backend reports what it supports through `HostFeatures`, and actions
whose requirements aren't met don't appear. A missing capability never
produces a broken button.

**Verification status** (2026-10-08). Linux X11 is exercised end to end under
Xvfb, including the ecosystem checks in Arcade Link's runner, and Lens runs
daily on Hyprland. CI builds, lints and tests on Linux, Windows and macOS;
the Windows and macOS backends have not been run interactively.
