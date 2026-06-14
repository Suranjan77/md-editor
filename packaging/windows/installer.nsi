Unicode True
RequestExecutionLevel user
!ifndef APP_VERSION
  !error "APP_VERSION required"
!endif
!ifndef SOURCE_DIR
  !error "SOURCE_DIR required"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE required"
!endif

Name "MD Editor"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\MD Editor"
InstallDirRegKey HKCU "Software\MD Editor" "InstallDir"
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "MD Editor"
  SetOutPath "$INSTDIR"
  File "${SOURCE_DIR}\md-editor.exe"
  File "${SOURCE_DIR}\md-editor.png"
  File "${SOURCE_DIR}\LICENSE"
  SetOutPath "$INSTDIR\resources"
  File "${SOURCE_DIR}\resources\pdfium.dll"
  SetOutPath "$INSTDIR\THIRD_PARTY_LICENSES"
  File /r "${SOURCE_DIR}\THIRD_PARTY_LICENSES\*"
  Delete "$INSTDIR\portable.flag"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\MD Editor" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MD Editor" "DisplayName" "MD Editor"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MD Editor" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MD Editor" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  CreateShortcut "$SMPROGRAMS\MD Editor.lnk" "$INSTDIR\md-editor.exe"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\MD Editor.lnk"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\MD Editor"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MD Editor"
SectionEnd
