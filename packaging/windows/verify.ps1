# Exercises the actual Inno install/uninstall path on an ephemeral CI account.
$ErrorActionPreference = 'Stop'

if (-not $env:RUNNER_TEMP) { throw 'RUNNER_TEMP is required; this check is for CI only.' }
$root = Join-Path $env:RUNNER_TEMP ('lens-install-' + [guid]::NewGuid().ToString('N'))
$env:ARCADE_HOME = Join-Path $root 'arcade'
$env:ARCADE_LENS_HOME = Join-Path $root 'lens'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\Arcade Lens'
$desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Arcade Lens.lnk'
$receiptFile = Join-Path $env:ARCADE_HOME 'installs\arcade.lens.json'
$fixtureReceipt = Join-Path $env:ARCADE_HOME 'installs\arcade.fixture.json'
$setupFiles = @(Get-ChildItem -Path 'dist\ArcadeLens-*-windows-x64-setup.exe' -File)
$receipt = $null
$helper = $null

function Get-PathEntryCount([string]$Target) {
    $rootKey = 'HKCU:\Software\ArcadeLink\Isolated'
    if (-not (Test-Path -LiteralPath $rootKey)) { return 0 }
    $count = 0
    foreach ($key in Get-ChildItem -LiteralPath $rootKey -ErrorAction SilentlyContinue) {
        $value = (Get-ItemProperty -LiteralPath $key.PSPath -Name Path -ErrorAction SilentlyContinue).Path
        if ($null -ne $value) {
            $count += @($value -split ';' | Where-Object {
                [string]::Equals($_.Trim().Trim('"').TrimEnd('\', '/'), $Target.TrimEnd('\', '/'), [StringComparison]::OrdinalIgnoreCase)
            }).Count
        }
    }
    return $count
}

try {
    if ($setupFiles.Count -ne 1) { throw "Expected one setup exe in dist, found $($setupFiles.Count)." }
    if (Test-Path -LiteralPath $installDir) { throw "Unexpected existing Lens install at $installDir." }
    if (Test-Path -LiteralPath $desktopShortcut) { throw "Unexpected existing desktop shortcut at $desktopShortcut." }
    New-Item -ItemType Directory -Force -Path $root | Out-Null

    $process = Start-Process -FilePath $setupFiles[0].FullName `
        -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /MERGETASKS=arcade_startup' `
        -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno install exited $($process.ExitCode)." }
    if (-not (Test-Path -LiteralPath $receiptFile)) { throw 'Silent install did not write a receipt.' }

    $receipt = Get-Content -LiteralPath $receiptFile -Raw | ConvertFrom-Json
    if ($receipt.method -ne 'windows-installer' -or $receipt.managedBy -ne 'self') { throw 'Wrong receipt method or owner.' }
    if ($receipt.version -ne $setupFiles[0].BaseName.Replace('ArcadeLens-', '').Replace('-windows-x64-setup', '')) { throw 'Receipt version differs from package name.' }
    if ($receipt.path -ine (Join-Path $installDir 'arcade-lens.exe')) { throw "Wrong per-user install path: $($receipt.path)." }
    if (-not (Test-Path -LiteralPath $receipt.path)) { throw 'Installed executable is missing.' }
    if (-not (Test-Path -LiteralPath $receipt.integration.desktopEntry)) { throw 'Start menu shortcut is missing.' }
    if (-not $receipt.integration.autostart -or -not (Test-Path -LiteralPath $receipt.integration.autostart)) { throw 'Selected login shortcut is missing.' }
    if (-not (Test-Path -LiteralPath $receipt.integration.cli)) { throw 'CLI shim is missing.' }
    if ($receipt.integration.desktopShortcut) { throw 'The unchecked desktop task was unexpectedly selected.' }
    if (Test-Path -LiteralPath $desktopShortcut) { throw 'The unchecked desktop shortcut was created.' }
    if ((Get-PathEntryCount (Join-Path $env:ARCADE_HOME 'bin')) -ne 1) { throw 'Shared Arcade bin was not added to isolated PATH exactly once.' }
    if (Get-Process -Name 'arcade-lens' -ErrorAction SilentlyContinue) { throw 'Silent install launched Lens.' }

    $metadata = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\arcade.lens'
    if ($metadata.DisplayName -ne 'Arcade Lens' -or $metadata.DisplayVersion -ne $receipt.version -or $metadata.Publisher -ne 'qa-p1') { throw 'Apps & features metadata is incorrect.' }
    if ($metadata.URLInfoAbout -ne 'https://github.com/qa-p1/Arcade-catalog') { throw 'Apps & features homepage is incorrect.' }

    $helper = Join-Path $root 'arcade-integration.ps1'
    Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $receipt.path) 'arcade-integration.ps1') -Destination $helper
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    & $powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File $helper `
        -Action Install -Id 'arcade.fixture' -CliName 'arcade-fixture' -Version '0.1.0' -ExePath $receipt.path
    if ($LASTEXITCODE -ne 0) { throw "Second app integration exited $LASTEXITCODE." }
    if ((Get-PathEntryCount (Join-Path $env:ARCADE_HOME 'bin')) -ne 1) { throw 'Second app duplicated the shared PATH entry.' }
    if (-not (Test-Path -LiteralPath (Join-Path $env:ARCADE_HOME 'bin\arcade-fixture.cmd'))) { throw 'Second app CLI shim is missing.' }

    $sentinel = Join-Path $env:ARCADE_LENS_HOME 'data\uninstall-preserves-this-file'
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $sentinel) | Out-Null
    Set-Content -LiteralPath $sentinel -Value 'keep'
    $process = Start-Process -FilePath $receipt.integration.uninstaller `
        -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno uninstall exited $($process.ExitCode)." }
    if (Test-Path -LiteralPath $receiptFile) { throw 'Uninstall kept the Lens receipt.' }
    if (Test-Path -LiteralPath $receipt.integration.cli) { throw 'Uninstall kept the Lens CLI shim.' }
    if (Test-Path -LiteralPath $receipt.integration.desktopEntry) { throw 'Uninstall kept the Start menu shortcut.' }
    if (Test-Path -LiteralPath $receipt.integration.autostart) { throw 'Uninstall kept the login shortcut.' }
    $settingsShortcut = Join-Path (Split-Path -Parent $receipt.integration.desktopEntry) 'Arcade Lens Settings.lnk'
    if (Test-Path -LiteralPath $settingsShortcut) { throw 'Uninstall kept the settings shortcut.' }
    if (-not (Test-Path -LiteralPath $sentinel)) { throw 'Uninstall removed Lens user data.' }
    if (-not (Test-Path -LiteralPath $fixtureReceipt)) { throw 'Uninstall removed another app receipt.' }
    if ((Get-PathEntryCount (Join-Path $env:ARCADE_HOME 'bin')) -ne 1) { throw 'Uninstall removed shared PATH while another app remains.' }
    if (Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\arcade.lens') { throw 'Uninstall kept Lens Apps & features metadata.' }

    & $powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File $helper `
        -Action Uninstall -Id 'arcade.fixture' -CliName 'arcade-fixture' -ExePath $receipt.path
    if ($LASTEXITCODE -ne 0) { throw "Last app cleanup exited $LASTEXITCODE." }
    if ((Get-PathEntryCount (Join-Path $env:ARCADE_HOME 'bin')) -ne 0) { throw 'Last app uninstall kept the shared PATH entry.' }
    Write-Output 'Windows CI install/uninstall passed: per-user location, receipt, startup task, unchecked desktop, one shared PATH entry, last-app removal and data preservation.'
} finally {
    if ((Test-Path -LiteralPath $fixtureReceipt) -and $helper) {
        try {
            $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
            & $powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File $helper `
                -Action Uninstall -Id 'arcade.fixture' -CliName 'arcade-fixture' -ExePath $receipt.path
        } catch { Write-Warning "Fixture integration cleanup failed: $_" }
    }
    if ((Test-Path -LiteralPath $receiptFile) -and $receipt) {
        try {
            Start-Process -FilePath $receipt.integration.uninstaller `
                -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait | Out-Null
        } catch { Write-Warning "Lens installer cleanup failed: $_" }
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
}
