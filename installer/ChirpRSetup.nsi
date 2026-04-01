!include "MUI2.nsh"

!define PRODUCT_VERSION "0.2.2"
!define DIST_PATH "${BUILD_ROOT}\dist\chirpr-windows-x64"

Name "ChirpR"
OutFile "ChirpRSetup.exe"
InstallDir "$PROGRAMFILES64\ChirpR"
InstallDirRegKey HKLM "Software\ChirpR" "InstallDir"
RequestExecutionLevel admin

!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${BUILD_ROOT}\installer\License.rtf"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

Function .onInstSuccess
    ExecWait 'cmd /c cd /d "$INSTDIR" && start "" "$INSTDIR\chirpr.exe"'
FunctionEnd

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
    ExecWait 'taskkill /F /IM chirpr.exe'
    
    SetOutPath "$INSTDIR"
    
    File "${DIST_PATH}\chirpr.exe"
    File "${DIST_PATH}\chirpr-cli.exe"
    File "${DIST_PATH}\config.toml"
    File "${DIST_PATH}\LICENSE"
    File "${DIST_PATH}\run-portable.cmd"
    File "${DIST_PATH}\install.ps1"
    File "${DIST_PATH}\uninstall.ps1"
    
    SetOutPath "$INSTDIR\assets"
    File /r "${DIST_PATH}\assets\*.*"
    
    WriteRegStr HKLM "Software\ChirpR" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR" "DisplayName" "ChirpR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR" "UninstallString" "powershell.exe -ExecutionPolicy Bypass -File uninst.tmp"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR" "DisplayVersion "${PRODUCT_VERSION}""
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR" "NoRepair" 1
    
    CreateDirectory "$SMPROGRAMS\ChirpR"
    CreateShortCut "$SMPROGRAMS\ChirpR\ChirpR.lnk" "$INSTDIR\chirpr.exe"
    CreateShortCut "$SMPROGRAMS\ChirpR\Uninstall ChirpR.lnk" "$INSTDIR\uninstall.exe"
    
    WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
    ExecWait 'taskkill /F /IM chirpr.exe'
    
    Delete "$INSTDIR\chirpr.exe"
    Delete "$INSTDIR\chirpr-cli.exe"
    Delete "$INSTDIR\config.toml"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\run-portable.cmd"
    Delete "$INSTDIR\install.ps1"
    Delete "$INSTDIR\uninstall.ps1"
    Delete "$INSTDIR\uninstall.exe"
    RMDir /r "$INSTDIR\assets"
    RMDir "$INSTDIR"
    
    Delete "$SMPROGRAMS\ChirpR\ChirpR.lnk"
    Delete "$SMPROGRAMS\ChirpR\Uninstall ChirpR.lnk"
    RMDir "$SMPROGRAMS\ChirpR"
    
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChirpR"
    DeleteRegKey HKLM "Software\ChirpR"
SectionEnd
