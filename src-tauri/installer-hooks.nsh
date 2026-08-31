; Tauri NSIS installer hooks.
;
; Force a Start Menu shortcut on every install — including the updater's silent
; reinstall, which otherwise leaves the app with no launcher at all once the
; window (which has no taskbar presence) is closed.

!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
!macroend
