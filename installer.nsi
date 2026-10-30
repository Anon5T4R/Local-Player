!include "MUI2.nsh"

Name "LocalPlayer"
OutFile "local-player-setup.exe"
InstallDir "$PROGRAMFILES\LocalPlayer"
RequestExecutionLevel admin

!define VERSION "0.9.4"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

; ESSENCIAL: sem isto as páginas MUI ficam em branco (sem strings de idioma).
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "target\release\local-player.exe"

  ; Atalhos no contexto do USUÁRIO (não do admin elevado).
  SetShellVarContext current

  ; Menu Iniciar
  CreateDirectory "$SMPROGRAMS\LocalPlayer"
  CreateShortCut "$SMPROGRAMS\LocalPlayer\LocalPlayer.lnk" "$INSTDIR\local-player.exe"
  ; Desktop
  CreateShortCut "$DESKTOP\LocalPlayer.lnk" "$INSTDIR\local-player.exe"

  ; Registro de desinstalação (Adicionar/Remover programas)
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "DisplayName" "LocalPlayer"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "Publisher" "Anon5T4R"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "DisplayIcon" "$INSTDIR\local-player.exe"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer" "NoRepair" 1

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  ; Remove atalhos no contexto do usuário
  SetShellVarContext current
  Delete "$SMPROGRAMS\LocalPlayer\LocalPlayer.lnk"
  Delete "$DESKTOP\LocalPlayer.lnk"
  RMDir "$SMPROGRAMS\LocalPlayer"

  ; Remove arquivos
  Delete "$INSTDIR\local-player.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  ; Remove registro
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LocalPlayer"
SectionEnd
