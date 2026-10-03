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
| `lens-recognizers` | Built-in recognizers: OCR integration, ~15 structured-text recognizers, colors/palettes, UI inspection, image/icon, window matching, QR/barcodes. |
| `lens-actions` | ~140 built-in actions and the default chains. |
| `arcade-lens` | The application: desktop `Host`, configuration, CLI harness. The overlay shell lands here next. |

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
  and ids must be namespaced under the plugin id.

### Ranking
`build_palette` scores every (action, finding) pair:

```
score = action priority
      + 40 × specificity(capability) × confidence     # url ≫ text ≫ region
      + learned usage boost (≤ 25, decaying, local)
      + 40 if the user pinned the action as preferred
```

The top N (default 5) distinct actions become the primary row (at most two
per group so the row isn't five "Copy" variants); everything else is the
`•••` list. Keys are assigned per (action, finding), primary entries first,
honoring user overrides. `stabilize` keeps already-visible entries in place
while late results arrive, so the palette doesn't jump.

### Usage learning
`UsageStore` keeps a decaying counter per (capability, action) — no content,
no account, resettable (`arcade-lens reset-usage`). Half-life is 30 days and
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
commands, download, local collections, measure mode. Unsupported services
are absent from `HostFeatures`, so actions needing them simply don't appear.

## Privacy

* Recognition is entirely local: OCR (ocrs/ONNX, or platform engines),
  QR/barcodes (rxing), all text recognizers (deterministic rules).
* Selections are ephemeral; nothing is persisted unless an action does so.
* `Value`'s `Debug` never prints secrets or pixels; `Sensitive` wraps raw
  secret material.
* Usage learning stores only capability → action counters.
* External services are only contacted by actions that inherently need them
  (search, maps, WHOIS, currency rates via search), with configurable
  provider templates and a visible preview of what is sent.

## Platform plan

Shared logic stays in the crates above. Platform adapters implement:

| Area | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Capture | Windows.Graphics.Capture / DXGI duplication | ScreenCaptureKit | XShm / XComposite | `xdg-desktop-portal` Screenshot |
| Global shortcut | RegisterHotKey | Carbon/CGEventTap | XGrabKey | portal GlobalShortcuts (or compositor binding) |
| Overlay | layered topmost windows per monitor | NSPanel per screen | override-redirect windows | layer-shell (wlroots/KDE) or portal-frozen fullscreen |
| OCR | Windows.Media.Ocr | Vision | ocrs | ocrs |
| Windows list | EnumWindows + DWM bounds | CGWindowList | _NET_CLIENT_LIST | compositor-specific / unavailable |

Wayland deliberately prevents global capture and global hotkeys without
user-granted portals; there the "freeze" is the portal screenshot itself,
and the activation shortcut is bound through the portal or the compositor.
