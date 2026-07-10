; AI Handoff NSIS installer hooks.
;
; The GUI package is self-contained: Tauri installs the CLI and native
; background host sidecars in $INSTDIR, and this hook provisions their managed
; copies under ~/.ai-handoff/bin. `ai-handoff install` then owns PATH, agent
; hooks, and the demand-only host registration. Renaming before CopyFiles keeps
; upgrades reliable even when the old host is finishing its idle period.

!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$SMPROGRAMS\aho.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"

  CreateDirectory "$PROFILE\.ai-handoff\bin"
  Delete "$PROFILE\.ai-handoff\bin\ai-handoff.exe.old"
  Delete "$PROFILE\.ai-handoff\bin\ai-handoff-host.exe.old"
  Rename "$PROFILE\.ai-handoff\bin\ai-handoff.exe" "$PROFILE\.ai-handoff\bin\ai-handoff.exe.old"
  Rename "$PROFILE\.ai-handoff\bin\ai-handoff-host.exe" "$PROFILE\.ai-handoff\bin\ai-handoff-host.exe.old"
  CopyFiles /SILENT "$INSTDIR\ai-handoff.exe" "$PROFILE\.ai-handoff\bin\ai-handoff.exe"
  CopyFiles /SILENT "$INSTDIR\ai-handoff-host.exe" "$PROFILE\.ai-handoff\bin\ai-handoff-host.exe"

  nsExec::ExecToLog '"$PROFILE\.ai-handoff\bin\ai-handoff.exe" install --yes'
  Pop $0
  StrCmp $0 "0" ai_handoff_install_ok
    MessageBox MB_ICONSTOP|MB_OK "AI Handoff CLI integration failed to install (exit code $0)."
    Abort
  ai_handoff_install_ok:

  ; Custom wizard pages do not run for /S or /P, so preserve config in both
  ; non-interactive modes. Interactive choices are fixed en/ko/ja values.
  IfSilent ai_handoff_config_done
  StrCmp $PassiveMode "1" ai_handoff_config_done

  nsExec::ExecToLog '"$PROFILE\.ai-handoff\bin\ai-handoff.exe" config set language "$AiHandoffUiLanguage"'
  Pop $0
  StrCmp $0 "0" ai_handoff_ui_language_ok
    MessageBox MB_ICONSTOP|MB_OK "AI Handoff application language failed to save (exit code $0)."
    Abort
  ai_handoff_ui_language_ok:

  nsExec::ExecToLog '"$PROFILE\.ai-handoff\bin\ai-handoff.exe" config set capsule.language "$AiHandoffCapsuleLanguage"'
  Pop $0
  StrCmp $0 "0" ai_handoff_config_done
    MessageBox MB_ICONSTOP|MB_OK "AI Handoff capsule language failed to save (exit code $0)."
    Abort
  ai_handoff_config_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove the on-demand task and agent hooks before Tauri deletes the bundled
  ; sidecars. Local capsule/store data is intentionally retained.
  nsExec::ExecToLog '"$INSTDIR\ai-handoff.exe" uninstall --keep-store --yes'
  Pop $0
  StrCmp $0 "0" ai_handoff_uninstall_ok
    MessageBox MB_ICONSTOP|MB_OK "AI Handoff CLI integration failed to uninstall (exit code $0). The GUI was kept so cleanup can be retried."
    Abort
  ai_handoff_uninstall_ok:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\aho.lnk"
!macroend
