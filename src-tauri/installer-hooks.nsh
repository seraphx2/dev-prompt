; Tauri NSIS installer hooks.
;
; Force a Start Menu shortcut on every install — including the updater's silent
; reinstall, which otherwise leaves the app with no launcher at all once the
; window (which has no taskbar presence) is closed.

!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Enable start-at-login on a fresh install, silently — the user opts out later
  ; in the app's Settings. Skipped when $UpdateMode is set so the updater's
  ; reinstall never resurrects a choice the user turned off. tauri-plugin-autostart
  ; reads and writes this exact value, so the in-app toggle stays in sync.
  ${If} $UpdateMode = 0
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostart'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"

  ; Start-at-login is registered by the app at runtime (tauri-plugin-autostart
  ; writes an HKCU Run value named after the product). The installer never
  ; created it, so nothing else removes it — left behind it points at a deleted
  ; exe and Windows flags a broken startup entry. Drop it, plus the companion
  ; Task-Manager toggle-state entry, on uninstall.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"

  ; The app writes its caches to <cache_dir>/cache, which on a currentUser
  ; install resolves under $INSTDIR. Tauri's uninstaller only removes files it
  ; recorded installing, so this runtime-written tree is left behind — clear it.
  RMDir /r "$INSTDIR\cache"
!macroend
