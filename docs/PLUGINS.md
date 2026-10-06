# Writing Arcade Lens plugins

A plugin adds recognizers (new things Lens can find in a selection) and
actions (new things it can do with them). Plugins run **out of process**: a
plugin is a directory holding a `plugin.toml` manifest and an executable that
speaks newline-delimited JSON over stdin/stdout. Any language works. The
reference examples are in [`examples/plugins`](../examples/plugins):

| Example | Shows |
|---|---|
| `isbn/` | A recognizer (ISBN-10/13 with checksum) plus two actions, one of them outbound |

Arcade Look, Box, Clipboard and Wheel use the built-in
[Arcade Link module](ARCADE_LINK.md). They are enabled by default when
installed; switch them off in **Settings → Connected apps**.

## Installing and enabling

Copy the plugin's directory into the plugins folder (**Settings → Plugins →
Open plugins folder**), then enable it in **Settings → Plugins** and save. A
discovered plugin never runs until the user enables it.

## Manifest (`plugin.toml`)

```toml
[plugin]
id = "dev.example.isbn"          # reverse-DNS; every id below must start with it
name = "ISBN lookup"
version = "1.0.0"
api_version = 1                  # must equal the plugin API version of this Lens build
description = "…"
executable = "isbn.py"           # relative to the plugin directory; *.py runs with python3
args = []                        # extra arguments for the executable
permissions = ["read-text", "clipboard", "network", "launch-apps"]

[[capabilities]]                 # new capabilities, with their is-a parent (default: text)
name = "dev.example.isbn"
parent = "text"

[[recognizers]]
id = "dev.example.isbn.detect"
consumes = ["text", "barcode"]   # findings that are offered to this recognizer
produces = ["dev.example.isbn"]  # the only capabilities it may return
cost = "trivial"                 # trivial | cheap (default) | expensive

[[actions]]
id = "dev.example.isbn.lookup"
label = "Look Up Book"
accepts = ["dev.example.isbn"]
produces = "nothing"             # nothing (default) | same | <capability>, used by chains
priority = 85                    # 0–90; built-in actions use the same scale
key = "b"                        # preferred palette key
group = "search"                 # copy | open | transform | save | share | search | inspect | edit
icon = "puzzle"
effects = ["network", "launch-apps"]
requires = ["open-uri"]          # host features: clipboard, clipboard-image, open-uri, open-path, save-file, terminal
```

A manifest that targets another API version, names an unknown permission
or effect, or uses ids outside its namespace is rejected, and
`plugins list` shows why.

## Permissions

There are two kinds of permission.

**Data permissions** decide what Lens sends to the plugin:

| Permission | Lens sends |
|---|---|
| `read-text` | the text of text-like findings |
| `read-pixels` | the selection pixels, as PNG |

A plugin without either never receives any selection content, and its
recognizers are never scheduled.

**Effect permissions** set the most an action may do. An action's `effects`
must be covered by them, or registration fails. They also decide which
effects Lens will carry out for the plugin: `clipboard`, `writes-files`,
`overwrites-files`, `deletes-files`, `network`, `uploads-content`,
`sends-to-device`, `launch-apps`, `executes-commands`, `privileged`,
`window-control`, `persists`.

Plugin actions go through the same safety policy as built-in ones:
- Outbound actions are marked ↗ and preview the text they send.
- The secret guard removes them when the selection contains a credential.
- Dangerous actions always ask for confirmation.

## Protocol

Lens starts the executable the first time a recognizer or action is needed,
then keeps it running. Each request is one JSON line on stdin, and each reply
is one JSON line on stdout with the same `id`. Lines without a matching `id`
are ignored, so print logs to stderr. The environment variable
`ARCADE_LENS_PLUGIN_API` holds the API version.

### `recognize` (3 s timeout)

```json
{"id": 1, "method": "recognize",
 "params": {"recognizer": "dev.example.isbn.detect",
            "input": {"capability": "text", "text": "ISBN 978-0-306-40615-7"}}}
```

```json
{"id": 1, "result": {"detections": [
  {"capability": "dev.example.isbn", "text": "9780306406157", "confidence": 0.95,
   "details": [["ISBN", "9780306406157"]], "data": {}}
]}}
```

Detections with capabilities the recognizer did not declare in `produces`
are dropped. Lens types the value when the capability is `url`, `email`,
`domain`, `address` or `text`, so built-in actions apply to it. Any other
capability becomes a custom value carrying `text` and `data`.

### `execute` (30 s timeout)

```json
{"id": 2, "method": "execute",
 "params": {"action": "dev.example.isbn.lookup",
            "input": {"capability": "dev.example.isbn", "text": "9780306406157"},
            "params": {}}}
```

```json
{"id": 2, "result": {
  "message": "Looking up 9780306406157",
  "effects": [{"type": "open-uri", "uri": "https://openlibrary.org/isbn/9780306406157"}],
  "output": {"capability": "text", "text": "…"}
}}
```

Plugins never call Lens services directly. They ask for **effects**, and Lens
performs each one only if the action declared the matching effect:

| Effect | Fields | Requires the action to declare |
|---|---|---|
| `copy-text` | `text` | `clipboard` |
| `open-uri` | `uri` | `launch-apps` or `network` |
| `save-file` | `name`, `base64`, `mime` | `writes-files` (Lens picks a non-clobbering path) |
| `notify` | `text` | — |

An undeclared effect fails the whole action with a "did not declare
permission" error. `output` is optional. When present, it feeds the next step
of a chain.

### Errors and lifetime

Reply `{"id": …, "error": "message"}` to report a failure. If the process
exits or misses a timeout, Lens kills it. The failure affects only that
recognizer or action, and the next call starts a fresh process.

## Security model, honestly stated

The permission system controls **what Lens gives the plugin** and **what
Lens does for it**. It is not an OS sandbox. The plugin executable runs with
the user's privileges like any other program they install. Enabling a plugin is
therefore an explicit trust decision, which is why plugins start disabled.
