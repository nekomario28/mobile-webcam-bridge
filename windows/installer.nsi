Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "${AMB_DELETE_MANIFEST}"

Name "Mobile Webcam ${AMB_VERSION} (experimental)"
OutFile "${AMB_INSTALLER}"
InstallDir "$PROGRAMFILES64\Mobile Webcam"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show
!define APP_KEY "Software\nekomario28\Mobile Webcam"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Mobile Webcam"
!define CLASS_PREFIX "Software\Classes\CLSID\{5C2CD55C-92AD-4999-8666-912BD3E700"
!define VIDEO_CATEGORY "Software\Classes\CLSID\{860BB310-5D01-11D0-BD3B-00A0C911CE86}\Instance"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Japanese"

; The filter shares a property class across all slots. Refuse conflicting
; installations before copying files or deleting the shared property class.
!macro CheckCameraOwnership VIEW BASE DLL
  SetRegView ${VIEW}
  StrCpy $0 ${BASE}
  IntOp $3 ${BASE} + 43
  loop_${VIEW}:
    IntFmt $1 "%02X" $0
    ReadRegStr $2 HKLM "${CLASS_PREFIX}$1}\InprocServer32" ""
    StrCmp $2 "" next_${VIEW}
    StrCmp $2 "$INSTDIR\virtual-camera\${DLL}" owned_${VIEW}
  conflict_${VIEW}:
    MessageBox MB_OK|MB_ICONSTOP "Another Unity Capture installation uses this camera slot. Remove it with its own uninstaller before continuing." /SD IDOK
    SetErrorLevel 2
    Abort
  owned_${VIEW}:
    IntOp $4 ${BASE} + 1
    ; Extra slots would keep depending on this DLL after slot 0 is removed.
    IntCmp $0 $4 next_${VIEW} next_${VIEW} conflict_${VIEW}
  next_${VIEW}:
    IntOp $0 $0 + 1
    IntCmp $0 $3 done_${VIEW} loop_${VIEW} done_${VIEW}
  done_${VIEW}:
!macroend

!macro RegisterCamera VIEW EXE DLL
  ClearErrors
  ExecWait '"${EXE}" /s "$INSTDIR\virtual-camera\${DLL}"' $0
  IfErrors failed_${VIEW}
  StrCmp $0 0 completed_${VIEW}
  failed_${VIEW}:
    MessageBox MB_OK|MB_ICONSTOP "Virtual camera registration failed (${VIEW}-bit). Close camera applications and run Setup again." /SD IDOK
    SetErrorLevel 3
    Abort
  completed_${VIEW}:
!macroend

!macro DeleteCameraKey KEY VALUE
  ReadRegStr $0 HKLM "${KEY}" "${VALUE}"
  ${If} $0 != ""
    ClearErrors
    DeleteRegKey HKLM "${KEY}"
    IfErrors uninstall_failed
  ${EndIf}
!macroend

!macro RemoveCamera VIEW CAMERA PROPERTY
  SetRegView ${VIEW}
  ; DllUnregisterServer sweeps 42 slots and can fail on an absent slot. Remove
  ; only the category instance and COM classes created by this installer.
  !insertmacro DeleteCameraKey "${VIDEO_CATEGORY}\{5C2CD55C-92AD-4999-8666-912BD3E700${CAMERA}}" "FriendlyName"
  !insertmacro DeleteCameraKey "${CLASS_PREFIX}${CAMERA}}" ""
  !insertmacro DeleteCameraKey "${CLASS_PREFIX}${PROPERTY}}" ""
!macroend

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "Mobile Webcam requires 64-bit Windows." /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
  SetRegView 64
  SetShellVarContext all
  ReadRegStr $0 HKLM "${APP_KEY}" "InstallDir"
  StrCmp $0 "" +2
    StrCpy $INSTDIR $0
FunctionEnd

Section "Mobile Webcam"
  SetRegView 64
  ReadRegStr $0 HKLM "${APP_KEY}" "InstallDir"
  StrCmp $0 "" check_folder
  StrCmp $0 $INSTDIR check_camera
    MessageBox MB_OK|MB_ICONSTOP "Uninstall the existing Mobile Webcam before changing its installation folder." /SD IDOK
    SetErrorLevel 2
    Abort
  check_folder:
    IfFileExists "$INSTDIR\*.*" 0 check_camera
    MessageBox MB_OK|MB_ICONSTOP "Choose an empty installation folder." /SD IDOK
    SetErrorLevel 2
    Abort
  check_camera:
  !insertmacro CheckCameraOwnership 64 16 UnityCaptureFilter64.dll
  !insertmacro CheckCameraOwnership 32 32 UnityCaptureFilter32.dll
  SetRegView 64
  ClearErrors
  SetOutPath "$INSTDIR"
  File /r "${AMB_STAGE}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "${APP_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "Mobile Webcam (experimental)"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${AMB_VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "nekomario28"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
  CreateDirectory "$SMPROGRAMS\Mobile Webcam"
  CreateShortcut "$SMPROGRAMS\Mobile Webcam\Mobile Webcam.lnk" "$INSTDIR\mobile-webcam.exe"
  CreateShortcut "$SMPROGRAMS\Mobile Webcam\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  IfErrors install_failed
  ${DisableX64FSRedirection}
  !insertmacro RegisterCamera 64 "$SYSDIR\regsvr32.exe" UnityCaptureFilter64.dll
  ${EnableX64FSRedirection}
  !insertmacro RegisterCamera 32 "$WINDIR\SysWOW64\regsvr32.exe" UnityCaptureFilter32.dll
  Goto install_done
  install_failed:
    MessageBox MB_OK|MB_ICONSTOP "Installation could not write its files or shortcuts. Close Mobile Webcam and run Setup again." /SD IDOK
    SetErrorLevel 3
    Abort
  install_done:
SectionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd

Section "Uninstall"
  !insertmacro CheckCameraOwnership 64 16 UnityCaptureFilter64.dll
  !insertmacro CheckCameraOwnership 32 32 UnityCaptureFilter32.dll
  !insertmacro RemoveCamera 64 10 11
  !insertmacro RemoveCamera 32 20 21
  ClearErrors
  !insertmacro DeleteApplicationFiles
  IfErrors uninstall_failed
  !insertmacro DeleteCameraFiles
  IfErrors uninstall_failed
  !insertmacro DeleteApplicationDirectories
  Delete "$SMPROGRAMS\Mobile Webcam\Mobile Webcam.lnk"
  Delete "$SMPROGRAMS\Mobile Webcam\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Mobile Webcam"
  SetRegView 64
  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  DeleteRegKey HKLM "${APP_KEY}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Goto uninstall_done
  uninstall_failed:
    MessageBox MB_OK|MB_ICONSTOP "Some application files are still in use. Close Mobile Webcam and camera applications, then run Uninstall again." /SD IDOK
    SetErrorLevel 3
    Abort
  uninstall_done:
SectionEnd
