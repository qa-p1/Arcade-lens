# Shared by arcade.iss and arcade.nsh. Windows PowerShell 5.1, no modules.
# Vendor all three files together and pin their SHA-256 hashes.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Install','Uninstall')][string]$Action,
    [Parameter(Mandatory=$true)][ValidatePattern('^arcade\.[a-z0-9][a-z0-9.-]*$')][string]$Id,
    [Parameter(Mandatory=$true)][ValidatePattern('^arcade-[a-z0-9-]+$')][string]$CliName,
    [string]$Version = '',
    [string]$Channel = 'stable',
    [ValidateSet('self','tools')][string]$ManagedBy = 'self',
    [string]$ExePath = '',
    [string]$DesktopEntry = '',
    [string]$DesktopShortcut = '',
    [string]$Autostart = '',
    [string]$Uninstaller = '',
    [string]$Name = '',
    [string]$Homepage = 'https://github.com/qa-p1/Arcade-catalog',
    [string]$QuietUninstallArgs = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
)
$ErrorActionPreference = 'Stop'
# Installers only see an exit code; keep the reason next to the script.
trap {
    try { [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'arcade-integration-error.log'), ($_ | Out-String)) } catch { }
    exit 1
}
$root = Join-Path $env:LOCALAPPDATA 'Arcade'
if ($env:ARCADE_HOME) { $root = [IO.Path]::GetFullPath($env:ARCADE_HOME) }
$bin = Join-Path $root 'bin'
$receipts = Join-Path $root 'installs'
$receiptPath = Join-Path $receipts ($Id + '.json')
$shim = Join-Path $bin ($CliName + '.cmd')
$utf8 = New-Object Text.UTF8Encoding($false)
# 'none' marks an unchecked task; '-' is accepted from older includes.
if ($Autostart -in @('none','-')) { $Autostart = '' }
if ($DesktopShortcut -in @('none','-')) { $DesktopShortcut = '' }

function Write-Atomic([string]$Path, [string]$Text, [bool]$Private = $false) {
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
    $stage = $Path + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
    $backup = $stage + '.backup'
    try {
        $stream = New-Object IO.FileStream($stage, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $bytes = $utf8.GetBytes($Text); $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
        if ($Private) {
            $acl = New-Object Security.AccessControl.FileSecurity
            $acl.SetAccessRuleProtection($true, $false)
            $user = [Security.Principal.WindowsIdentity]::GetCurrent().User
            $system = New-Object Security.Principal.SecurityIdentifier('S-1-5-18')
            foreach ($sid in @($user, $system)) {
                $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'Allow')
                $acl.AddAccessRule($rule)
            }
            [IO.File]::SetAccessControl($stage, $acl)
        }
        # Windows PowerShell 5.1 may bind a null string argument as an empty
        # path in File.Replace. A uniquely owned backup works on both .NETs.
        if ([IO.File]::Exists($Path)) { [IO.File]::Replace($stage, $Path, $backup, $true) }
        else { [IO.File]::Move($stage, $Path) }
    } finally {
        if ([IO.File]::Exists($stage)) { [IO.File]::Delete($stage) }
        if ([IO.File]::Exists($backup)) { [IO.File]::Delete($backup) }
    }
}

function Same-PathEntry([string]$Entry) {
    $expanded = [Environment]::ExpandEnvironmentVariables($Entry.Trim().Trim('"')).TrimEnd('\','/')
    return [string]::Equals($expanded, $bin.TrimEnd('\','/'), [StringComparison]::OrdinalIgnoreCase)
}

function Isolated-EnvironmentKey([string]$Root) {
    if (-not ('ArcadePathIdentity' -as [type])) {
        Add-Type @'
using System;
using System.Text;
using System.Globalization;
public static class ArcadePathIdentity {
  public static string Key(string root) {
    var bytes = Encoding.UTF8.GetBytes(root.Replace('\\','/').TrimEnd('/'));
    ulong hash = 0xcbf29ce484222325UL;
    unchecked {
      foreach (byte input in bytes) {
        byte value = input >= 65 && input <= 90 ? (byte)(input + 32) : input;
        hash = (hash ^ value) * 0x100000001b3UL;
      }
    }
    return @"Software\ArcadeLink\Isolated\" + (hash & 0xffffffffffffUL).ToString("x12", CultureInfo.InvariantCulture);
  }
}
'@
    }
    return [ArcadePathIdentity]::Key($Root)
}

function Update-UserPath([bool]$Add) {
    $environmentKey = 'Environment'
    if ($env:ARCADE_HOME) {
        $environmentKey = Isolated-EnvironmentKey $root
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($environmentKey)
    try {
        $path = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        if ($key.GetValueNames() -contains 'Path') { $kind = $key.GetValueKind('Path') }
        if ($kind -notin @([Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString)) { throw 'User Path is not a string.' }
        $entries = @($path.Split(';'))
        if ($Add) {
            if (@($entries | Where-Object { Same-PathEntry $_ }).Count -gt 0) { return }
            $next = if ($path) { $path.TrimEnd(';') + ';' + $bin } else { $bin }
        } else {
            # Last app out: any other .cmd shim keeps the shared folder on PATH.
            if (@(Get-ChildItem -LiteralPath $bin -Filter '*.cmd' -File -ErrorAction SilentlyContinue).Count -gt 0) { return }
            $next = (@($entries | Where-Object { -not (Same-PathEntry $_) }) -join ';')
        }
        if ($next -eq $path) { return }
        $key.SetValue('Path', $next, $kind)
    } finally { $key.Dispose() }
    if ($environmentKey -ne 'Environment') { return }
    if (-not ('ArcadeEnvironmentBroadcast' -as [type])) {
        Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ArcadeEnvironmentBroadcast {
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint msg, UIntPtr wp,
    string lp, uint flags, uint timeout, out UIntPtr result);
}
'@
    }
    $result = [UIntPtr]::Zero
    [ArcadeEnvironmentBroadcast]::SendMessageTimeout([IntPtr]0xffff, 0x1a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
}

if ($Action -eq 'Install') {
    if (-not $Version -or -not [IO.File]::Exists($ExePath)) { throw 'Version and an existing executable are required.' }
    $ExePath = [IO.Path]::GetFullPath($ExePath)
    if ($ExePath.Contains('"') -or $ExePath.Contains("`n") -or $ExePath.Contains("`r")) { throw 'Invalid executable path.' }
    $old = $null
    if ([IO.File]::Exists($receiptPath)) { $old = Get-Content -Raw -LiteralPath $receiptPath -Encoding UTF8 | ConvertFrom-Json }
    $now = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
    $installedAt = if ($old -and $old.id -eq $Id -and $old.installedAt) { $old.installedAt } else { $now }
    # % is escaped for cmd.exe; quote paths, forward the original arguments.
    Write-Atomic $shim ("@echo off`r`n`"" + $ExePath.Replace('%','%%') + "`" %*`r`n")
    $receipt = [ordered]@{
        schema = 1; id = $Id; version = $Version; channel = $Channel
        method = 'windows-installer'; managedBy = $ManagedBy; path = $ExePath
        integration = [ordered]@{
            desktopEntry = $(if ($DesktopEntry) { $DesktopEntry } else { $null })
            desktopShortcut = $(if ($DesktopShortcut) { $DesktopShortcut } else { $null })
            icons = @(); cli = $shim
            autostart = $(if ($Autostart) { $Autostart } else { $null })
            uninstaller = $(if ($Uninstaller) { $Uninstaller } else { $null })
        }
        installedAt = $installedAt; updatedAt = $now
    }
    Write-Atomic $receiptPath ($receipt | ConvertTo-Json -Depth 5) $true
    Update-UserPath $true
    if ($Name) {
        $metadata = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $Id)
        try {
            foreach ($entry in @(@('DisplayName',$Name),@('DisplayVersion',$Version),@('Publisher','qa-p1'),@('URLInfoAbout',$Homepage),@('InstallLocation',[IO.Path]::GetDirectoryName($ExePath)),@('DisplayIcon',$ExePath),@('UninstallString',('"' + $Uninstaller + '"')))) {
                $metadata.SetValue($entry[0],$entry[1],[Microsoft.Win32.RegistryValueKind]::String)
            }
            $metadata.SetValue('NoModify',1,[Microsoft.Win32.RegistryValueKind]::DWord)
            $metadata.SetValue('NoRepair',1,[Microsoft.Win32.RegistryValueKind]::DWord)
            $metadata.SetValue('QuietUninstallString',('"' + $Uninstaller + '" ' + $QuietUninstallArgs),[Microsoft.Win32.RegistryValueKind]::String)
        } finally { $metadata.Dispose() }
    }
} else {
    # Match ownership before removing a shim, so uninstalling an old copy
    # cannot remove a successor installation's shim/receipt.
    if ([IO.File]::Exists($receiptPath)) {
        $receipt = Get-Content -Raw -LiteralPath $receiptPath -Encoding UTF8 | ConvertFrom-Json
        if ($receipt.id -ne $Id) { throw 'Receipt id differs from filename.' }
        if (-not $ExePath -or -not [string]::Equals($receipt.path, [IO.Path]::GetFullPath($ExePath), [StringComparison]::OrdinalIgnoreCase)) { exit 10 }
        $entry = $receipt.integration.cli
        if ($entry -and [IO.File]::Exists($entry)) {
            # A replaced shim is foreign even if its path is still recorded.
            $expected = '@echo off' + "`r`n" + '"' + $receipt.path.Replace('%','%%') + '" %*' + "`r`n"
            if ([IO.File]::ReadAllText($entry) -ceq $expected) { [IO.File]::Delete($entry) }
            else { Write-Warning ('Preserving changed CLI shim: ' + $entry) }
        }
        $shell = New-Object -ComObject WScript.Shell
        try {
            foreach ($entry in @($receipt.integration.desktopEntry, $receipt.integration.desktopShortcut, $receipt.integration.autostart)) {
                if (-not $entry -or -not [IO.File]::Exists($entry)) { continue }
                if (-not [IO.Path]::IsPathRooted($entry) -or [IO.Path]::GetExtension($entry) -ine '.lnk') {
                    Write-Warning ('Preserving invalid shortcut path: ' + $entry)
                    continue
                }
                $shortcut = $null
                try {
                    $shortcut = $shell.CreateShortcut($entry)
                    if ([string]::Equals([IO.Path]::GetFullPath($shortcut.TargetPath), [IO.Path]::GetFullPath($receipt.path), [StringComparison]::OrdinalIgnoreCase)) {
                        [IO.File]::Delete($entry)
                        if ($entry -eq $receipt.integration.desktopEntry) {
                            $parent = [IO.Path]::GetDirectoryName($entry)
                            if ([IO.Directory]::GetFileSystemEntries($parent).Length -eq 0) { [IO.Directory]::Delete($parent) }
                        }
                    } else { Write-Warning ('Preserving changed shortcut: ' + $entry) }
                } catch { Write-Warning ('Preserving unreadable shortcut: ' + $entry + ': ' + $_.Exception.Message) }
                finally { if ($shortcut) { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut) | Out-Null } }
            }
        } finally { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) | Out-Null }
        [IO.File]::Delete($receiptPath)
    } else { exit 10 } # No receipt means no proof of shared ownership.
    Update-UserPath $false
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $Id, $false)
    exit 0
}
