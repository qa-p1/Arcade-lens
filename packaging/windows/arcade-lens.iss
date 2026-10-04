; Arcade Lens installer (Inno Setup 6).
;
;   iscc /DAppVersion=0.1.0 /DSourceExe=C:\path\to\arcade-lens.exe packaging\windows\arcade-lens.iss
;
; Installs for the current user (no administrator rights needed), adds a
; Start menu entry, optionally starts Lens at sign-in, and launches it.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\arcade-lens.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
AppId={{6F1D2B7E-3C4A-4E8B-9F21-8A5C0D7E4B13}
AppName=Arcade Lens
AppVersion={#AppVersion}
AppVerName=Arcade Lens {#AppVersion}
AppPublisher=Arcade
AppPublisherURL=https://github.com/qa-p1/Arcade-lens
AppSupportURL=https://github.com/qa-p1/Arcade-lens/issues
DefaultDirName={autopf}\Arcade Lens
DefaultGroupName=Arcade Lens
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=ArcadeLens-{#AppVersion}-windows-x64-setup
SetupIconFile=..\icons\arcade-lens.ico
UninstallDisplayIcon={app}\arcade-lens.exe
UninstallDisplayName=Arcade Lens
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "startup"; Description: "Start Arcade Lens when I sign in"; GroupDescription: "Startup:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "arcade-lens.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Arcade Lens"; Filename: "{app}\arcade-lens.exe"; Comment: "Select anything on screen and act on it"
Name: "{autodesktop}\Arcade Lens"; Filename: "{app}\arcade-lens.exe"; Tasks: desktopicon

[Registry]
; The same login item Lens writes itself (Settings → Start at login).
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "ArcadeLens"; ValueData: """{app}\arcade-lens.exe"" --background"; Tasks: startup

[Run]
Filename: "{app}\arcade-lens.exe"; Description: "{cm:LaunchProgram,Arcade Lens}"; Flags: nowait postinstall skipifsilent
; Silent installs (updates) bring Lens back in the background.
Filename: "{app}\arcade-lens.exe"; Parameters: "--background"; Flags: nowait; Check: WizardSilent

[Code]
// Asks a running Lens to quit, so its executable can be replaced or removed.
procedure QuitRunningLens();
var
  Exe: String;
  Code: Integer;
begin
  Exe := ExpandConstant('{app}\arcade-lens.exe');
  if FileExists(Exe) then
  begin
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(1000);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitRunningLens();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Config: String;
begin
  if CurStep = ssPostInstall then
  begin
    // The installer decides about start at login, not Lens's first run.
    Config := ExpandConstant('{userappdata}\Arcade\Arcade Lens\config');
    ForceDirectories(Config);
    SaveStringToFile(Config + '\installed', '', False);
    if not WizardIsTaskSelected('startup') then
      RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', 'ArcadeLens');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    QuitRunningLens();
    RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', 'ArcadeLens');
  end;
end;
