; Per-user Inno Setup 6 installer. Shared receipt, PATH shim and tasks live in
; arcade.iss and arcade-integration.ps1, vendored from Arcade Link.
;
;   iscc /DAppVersion=0.1.1 /DSourceExe=C:\path\to\arcade-lens.exe \
;     /DOutputDir=C:\path\to\dist packaging\windows\arcade-lens.iss

#ifndef AppVersion
  #define AppVersion "0.1.1"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\arcade-lens.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define ARCADE_ID "arcade.lens"
#define ARCADE_NAME "Arcade Lens"
#define ARCADE_VERSION AppVersion
#define ARCADE_EXE "arcade-lens.exe"
#define ARCADE_CLI "arcade-lens"
; Keep the existing Inno upgrade/uninstall identity.
#define ARCADE_INSTALLER_ID "{{6F1D2B7E-3C4A-4E8B-9F21-8A5C0D7E4B13}"
#define ARCADE_HOMEPAGE "https://github.com/qa-p1/Arcade-catalog"
#define ARCADE_START_AT_LOGIN_DEFAULT 1
#define ARCADE_CUSTOM_EVENTS
#include "arcade.iss"

[Setup]
DisableDirPage=yes
DisableReadyPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=ArcadeLens-{#AppVersion}-windows-x64-setup
SetupIconFile=..\icons\arcade-lens.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "arcade-lens.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Arcade Lens\Arcade Lens Settings"; Filename: "{app}\arcade-lens.exe"; Parameters: "--settings"; Flags: uninsneveruninstall

[Run]
Filename: "{app}\arcade-lens.exe"; Description: "{cm:LaunchProgram,Arcade Lens}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The manifest and settings shortcut are shared integration. An old
; uninstaller removes them only while its receipt still owns the install.
Type: files; Name: "{code:LensArcadeRoot}\apps\arcade.lens.json"; Check: ArcadeOwnsSharedIntegration
Type: files; Name: "{userprograms}\Arcade Lens\Arcade Lens Settings.lnk"; Check: ArcadeOwnsSharedIntegration
Type: dirifempty; Name: "{userprograms}\Arcade Lens"; Check: ArcadeOwnsSharedIntegration

[Code]
var
  LensOwnsIntegration: Boolean;

// Mirrors arcade-integration.ps1: ARCADE_HOME isolates the shared root.
function LensArcadeRoot(Param: String): String;
begin
  Result := GetEnv('ARCADE_HOME');
  if Result = '' then Result := ExpandConstant('{localappdata}\Arcade');
end;

function ArcadeOwnsSharedIntegration: Boolean;
begin
  Result := LensOwnsIntegration;
end;

function GetCustomSetupExitCode: Integer;
begin
  if ArcadeIntegrationFailed then Result := 1 else Result := 0;
end;

procedure QuitRunningLens();
var
  Exe: String;
  ExitCode: Integer;
begin
  Exe := ExpandConstant('{app}\arcade-lens.exe');
  if FileExists(Exe) then
  begin
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ExitCode);
    Sleep(1000);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitRunningLens();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    ArcadeIntegration(True);
    // Remove the legacy first-run Run value; new installers use the shared
    // Start at login shortcut task.
    RegDeleteValue(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', 'ArcadeLens');
  end;
end;

// The helper deletes the receipt only when this copy owns it (exit 0); exit 10
// leaves a successor's receipt in place, so its files are kept too.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Receipt: String;
  HadReceipt: Boolean;
begin
  if CurUninstallStep = usUninstall then
  begin
    Receipt := LensArcadeRoot('') + '\installs\arcade.lens.json';
    HadReceipt := FileExists(Receipt);
    ArcadeIntegration(False);
    LensOwnsIntegration := HadReceipt and not FileExists(Receipt);
    if ArcadeOwnsSharedIntegration then
    begin
      QuitRunningLens();
      RegDeleteValue(HKEY_CURRENT_USER,
        'Software\Microsoft\Windows\CurrentVersion\Run', 'ArcadeLens');
    end;
  end;
end;
