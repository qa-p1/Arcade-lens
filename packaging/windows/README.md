# Shared per-user Windows integration

Vendor `arcade.iss`, `arcade.nsh` and `arcade-integration.ps1` from the same
Arcade Link tag. Record the tag and SHA-256 of **each file** in the app's
`VENDORED`; verify those hashes in CI, as for `qt/`. The PowerShell helper is
embedded as an installer payload; it requires only Windows PowerShell 5.1.
No helper download or third-party PowerShell module is involved.

Both include paths install under `%LOCALAPPDATA%\Programs\<Name>`, create a
Start menu entry, offer unchecked desktop and start-at-login tasks (apps may
set their existing startup default), and write a CLI shim and atomic receipt.
Receipts live at `%LOCALAPPDATA%\Arcade\installs\<id>.json`, matching Link's
Windows registry root. Native helper files restrict the receipt ACL to the
user and SYSTEM; Unix `0600` has no literal Windows equivalent.

The helper preserves the existing HKCU `Environment\Path` value type and
unexpanded contents. The bin directory is added once, compared without case,
and a `WM_SETTINGCHANGE` broadcast follows changes. Other `.cmd` shims keep
the folder on PATH during uninstall. The last one removes just that entry.
Neither installer launches the app on a silent path. Neither needs elevation,
changes machine PATH, or bypasses SmartScreen.

With `ARCADE_HOME`, tests use the isolated filesystem root and HKCU
`Software\ArcadeLink\Isolated\<hash>` instead of native user integration.
Rust, Qt and PowerShell share the FNV-1a identity specified in
[SPEC §8.6](https://github.com/qa-p1/Arcade-Link/blob/main/SPEC.md#86-install-standard), with fixed cross-language
[vectors](https://github.com/qa-p1/Arcade-Link/blob/main/spec/vectors/windows-path-identity.json). Receipt operations
still require native absolute paths; the hash is only an identity.

Define these before including either file:

| Define | Example |
| --- | --- |
| `ARCADE_ID` | `arcade.find` |
| `ARCADE_NAME` | `Arcade Find` |
| `ARCADE_VERSION` | `0.3.0` |
| `ARCADE_EXE` | `arcade-find.exe` (filename under install dir) |
| `ARCADE_CLI` | `arcade-find` |
| `ARCADE_HOMEPAGE` (optional) | catalog homepage URL |
| `ARCADE_START_AT_LOGIN_DEFAULT` (optional) | `0` or `1` (default `0`) |

Names/values must be trusted compile-time strings without quotes or newlines.
Keep existing installer identities: Inno's optional `ARCADE_INSTALLER_ID`
defaults to `ARCADE_ID`; override it with the app's current `AppId`.
The include disables Inno's automatic uninstall registry key and writes
receipt-owned HKCU Apps & features metadata under `ARCADE_ID`. Apps migrating
an older `_is1` key should remove that legacy key only while they own it.

For Inno, include `arcade.iss`, then supply the app's `[Files]` and build
directives. Existing Pascal event handlers define `ARCADE_CUSTOM_EVENTS`
and call `ArcadeIntegration(True)` at `ssPostInstall` and
`ArcadeIntegration(False)` at `usUninstall`. `/VERYSILENT /SUPPRESSMSGBOXES
/NORESTART` uses the same integration; `/MERGETASKS=arcade_startup` or
`/TASKS=` chooses startup tasks. Runtime `/ARCADEMANAGEDBY=tools` and
`/ARCADECHANNEL=stable` carry receipt ownership/channel.

For Unicode NSIS 3, insert `ArcadeSetup` for metadata/per-user defaults,
`ArcadeTasks` **before the main application section**, `ArcadeInstall` after
the application's `File` statements, `ArcadeInitTasks` in `.onInit`, and
`ArcadeUninstall` before deleting application files. Add the components page
to show the optional tasks. `/S /STARTATLOGIN=0|1 /DESKTOPSHORTCUT=0|1` is
silent parity; `/ARCADEMANAGEDBY=tools /ARCADECHANNEL=stable` works here too.
The includes preserve settings and data; the app owns any explicit remove-data
choice and must not recursively remove a data-bearing install directory.

`samples/` provides minimal compilable callers and an isolated CI-account
verification script. The `windows-installers` CI job builds and runs both,
checks repeated installs, two simultaneous shims, metadata, receipts, and
last-one-out removal. This job has not been run locally on Linux.
It also installs version 0.3.1 in a different directory and runs the old
0.3.0 uninstaller, checking that the successor's shortcuts, shim, receipt,
PATH entry and metadata survive. The shared helper returns exit 10 when the
receipt belongs to another executable (or is missing); both includes treat
that as a successful private uninstall and skip every shared cleanup.
Custom Inno uninstall handlers must still call `ArcadeIntegration(False)`
at `usUninstall` so the guarded `[UninstallDelete]` entries know ownership.

Implementation references: [Inno Pascal support functions](https://jrsoftware.org/ishelp/topic_scriptfunctions.htm),
[NSIS scripting reference](https://nsis.sourceforge.io/Docs/Chapter4.html).
