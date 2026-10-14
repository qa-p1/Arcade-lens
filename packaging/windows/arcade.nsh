; Unicode NSIS 3 include. Vendor with arcade-integration.ps1.
!ifndef ARCADE_NSH_INCLUDED
!define ARCADE_NSH_INCLUDED
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"
!ifndef ARCADE_HOMEPAGE
!define ARCADE_HOMEPAGE "https://github.com/qa-p1/Arcade-catalog"
!endif
!ifndef ARCADE_START_AT_LOGIN_DEFAULT
!define ARCADE_START_AT_LOGIN_DEFAULT 0
!endif
!define ARCADE_HELPER "${__FILEDIR__}\arcade-integration.ps1"
Var ArcadeStartup
Var ArcadeDesktop
Var ArcadeChannel
Var ArcadeManagedBy
Var ArcadeExitCode
Var ArcadeIntegrationOutput

!macro ArcadeSetup
  Unicode true
  RequestExecutionLevel user
  Name "${ARCADE_NAME}"
  InstallDir "$LOCALAPPDATA\Programs\${ARCADE_NAME}"
  InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "InstallLocation"
!macroend

; Insert before the main install section. Components page displays both tasks.
!macro ArcadeTasks
  Section /o "Desktop shortcut" ArcadeDesktopTask
    SetShellVarContext current
    CreateShortcut "$DESKTOP\${ARCADE_NAME}.lnk" "$INSTDIR\${ARCADE_EXE}"
  SectionEnd
  Section /o "Start at login" ArcadeStartupTask
    SetShellVarContext current
    CreateShortcut "$SMSTARTUP\${ARCADE_NAME}.lnk" "$INSTDIR\${ARCADE_EXE}" "--background"
  SectionEnd
!macroend

; Insert in .onInit after ArcadeTasks has declared its section IDs. /S uses
; the same defaults; /STARTATLOGIN=0|1 and /DESKTOPSHORTCUT=0|1 override them.
!macro ArcadeInitTasks
  SetShellVarContext current
  StrCpy $0 "${ARCADE_START_AT_LOGIN_DEFAULT}"
  ${GetParameters} $1
  ClearErrors
  ${GetOptions} $1 "/STARTATLOGIN=" $2
  ${IfNot} ${Errors}
    StrCpy $0 $2
  ${EndIf}
  ${If} $0 == "1"
    !insertmacro SelectSection ${ArcadeStartupTask}
  ${Else}
    !insertmacro UnselectSection ${ArcadeStartupTask}
  ${EndIf}
  ClearErrors
  ${GetOptions} $1 "/DESKTOPSHORTCUT=" $2
  ${IfNot} ${Errors}
    ${If} $2 == "1"
      !insertmacro SelectSection ${ArcadeDesktopTask}
    ${Else}
      !insertmacro UnselectSection ${ArcadeDesktopTask}
    ${EndIf}
  ${EndIf}
!macroend

; Called after the caller's application File statements. Powershell helper
; performs atomic JSON receipt and user Path operations without NSIS string
; truncation. Installer/uninstaller never launch the installed app silently.
!macro ArcadeInstall
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "${ARCADE_HELPER}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\${ARCADE_NAME}"
  CreateShortcut "$SMPROGRAMS\${ARCADE_NAME}\${ARCADE_NAME}.lnk" "$INSTDIR\${ARCADE_EXE}"
  StrCpy $ArcadeStartup "none"
  StrCpy $ArcadeDesktop "none"
  ${If} ${SectionIsSelected} ${ArcadeDesktopTask}
    StrCpy $ArcadeDesktop "$DESKTOP\${ARCADE_NAME}.lnk"
  ${EndIf}
  ; Section IDs are resolved after preprocessing; callers place ArcadeTasks
  ; before this section or use the two-stage sample below.
  ${If} ${SectionIsSelected} ${ArcadeStartupTask}
    StrCpy $ArcadeStartup "$SMSTARTUP\${ARCADE_NAME}.lnk"
  ${EndIf}
  StrCpy $ArcadeChannel "stable"
  StrCpy $ArcadeManagedBy "self"
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/ARCADECHANNEL=" $1
  ${IfNot} ${Errors}
    StrCpy $ArcadeChannel $1
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/ARCADEMANAGEDBY=" $1
  ${IfNot} ${Errors}
    StrCpy $ArcadeManagedBy $1
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "$INSTDIR\arcade-integration.ps1" -Action Install -Id "${ARCADE_ID}" -CliName "${ARCADE_CLI}" -Version "${ARCADE_VERSION}" -Channel "$ArcadeChannel" -ManagedBy "$ArcadeManagedBy" -ExePath "$INSTDIR\${ARCADE_EXE}" -DesktopEntry "$SMPROGRAMS\${ARCADE_NAME}\${ARCADE_NAME}.lnk" -DesktopShortcut "$ArcadeDesktop" -Autostart "$ArcadeStartup" -Uninstaller "$INSTDIR\uninstall.exe"'
  Pop $ArcadeExitCode
  Pop $ArcadeIntegrationOutput
  ${If} $ArcadeExitCode != 0
    FileOpen $0 "$INSTDIR\arcade-integration-error.log" w
    FileWrite $0 $ArcadeIntegrationOutput
    FileClose $0
    SetErrorLevel $ArcadeExitCode
    Abort "Arcade installation integration failed."
  ${EndIf}
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "DisplayName" "${ARCADE_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "DisplayVersion" "${ARCADE_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "Publisher" "qa-p1"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "URLInfoAbout" "${ARCADE_HOMEPAGE}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "DisplayIcon" "$INSTDIR\${ARCADE_EXE}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${ARCADE_ID}" "NoRepair" 1
!macroend

!macro ArcadeUninstall
  SetShellVarContext current
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "$INSTDIR\arcade-integration.ps1" -Action Uninstall -Id "${ARCADE_ID}" -CliName "${ARCADE_CLI}" -ExePath "$INSTDIR\${ARCADE_EXE}"'
  Pop $ArcadeExitCode
  Pop $ArcadeIntegrationOutput
  ${If} $ArcadeExitCode == 10
    ; An old copy may remove its private payload, never successor integration.
    DetailPrint "Shared integration belongs to a successor; leaving it intact."
  ${ElseIf} $ArcadeExitCode != 0
    SetErrorLevel $ArcadeExitCode
    Abort "Arcade uninstall integration failed."
  ${EndIf}
  ; The helper removes only receipt-recorded shortcuts whose current target
  ; still matches this executable. Never guess unchecked task filenames.
  Delete "$INSTDIR\arcade-integration.ps1"
  Delete "$INSTDIR\arcade-integration-error.log"
!macroend
!endif
