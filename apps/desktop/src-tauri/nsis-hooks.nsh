; AI Handoff NSIS installer hooks.
;
; Tauri's NSIS bundle creates a Start Menu shortcut named after the product
; ("AI Handoff.lnk"), so the app is searchable by "AI Handoff". We add a second
; top-level shortcut named "aho.lnk" pointing at the same executable so it is
; also searchable by typing "aho". Removed again on uninstall.

!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$SMPROGRAMS\aho.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\aho.lnk"
!macroend
