; Per-user installer for Claude RPC. scripts/build.js runs it as:
;   makensis /DVERSION=x.y.z /DSOURCE=<built exe> /DOUTFILE=<setup exe> installer/windows.nsi
; Install folder, Start menu shortcut and uninstall key match the ones in
; Tauri 2's NSIS template (currentUser mode), so it upgrades 4.0.x installs in place.
;
; The 4.0.x updater runs it as `setup.exe /P /R /UPDATE /ARGS`:
;   /P  passive: no question, the progress page only
;   /S  silent: no window at all (NSIS built-in)
;   /R  start the app once installed
Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define PRODUCT "Claude RPC"
!define EXE "claude-rpc.exe"
!define PUBLISHER "inerthel-agi"
!define APP_ID "eu.inerthel.claude-rpc"
!define OLD_APP_ID "eu.stealthylabs.claude-rpc"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define NOTIFY_KEY "Software\Classes\AppUserModelId\${APP_ID}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "inerthel, MIT license"

Var Passive
Var Relaunch

!define MUI_ICON "..\logo\clawd.ico"
!define MUI_UNICON "..\logo\clawd.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--settings"

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    StrCpy $Relaunch 1
  ${EndIf}
  ; An earlier install elsewhere (the Tauri installer let you pick the folder).
  ReadRegStr $1 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $1 != ""
    ; Tauri stored the folder in quotes.
    StrCpy $2 $1 1
    ${If} $2 == '"'
      StrCpy $1 $1 "" 1
      StrCpy $1 $1 -1
    ${EndIf}
    StrCpy $INSTDIR $1
  ${EndIf}
FunctionEnd

Function SkipWhenPassive
  ${If} $Passive == 1
    Abort
  ${EndIf}
FunctionEnd

; The running app keeps its executable locked.
!macro StopApp
  nsExec::Exec 'taskkill /F /IM "${EXE}"'
  Pop $0
  Sleep 500
!macroend

Section "Install"
  !insertmacro StopApp
  ; Web view data of the Tauri builds (4.0.5 and older); nothing reads it any more.
  RMDir /r "$LOCALAPPDATA\${OLD_APP_ID}\EBWebView"
  RMDir "$LOCALAPPDATA\${OLD_APP_ID}"
  SetOutPath "$INSTDIR"
  File "/oname=${EXE}" "${SOURCE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "--settings"
  ; Lets Windows attribute the app's notifications to it.
  WriteRegStr HKCU "${NOTIFY_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${NOTIFY_KEY}" "IconUri" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${EXE}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" '"$INSTDIR"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "MainBinaryName" "${EXE}"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
  ${If} $Relaunch == 1
    Exec '"$INSTDIR\${EXE}"'
  ${EndIf}
SectionEnd

; Settings, status and usage history in %USERPROFILE%\.claude-rpc are kept.
Section "Uninstall"
  !insertmacro StopApp
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  ; The Tauri installers could also leave a desktop shortcut.
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "${PRODUCT}"
  DeleteRegValue HKCU "${APPROVED_KEY}" "${PRODUCT}"
  DeleteRegKey HKCU "${NOTIFY_KEY}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; Written by the Tauri installers.
  DeleteRegKey HKCU "Software\${PUBLISHER}\${PRODUCT}"
  DeleteRegKey /ifempty HKCU "Software\${PUBLISHER}"
SectionEnd
