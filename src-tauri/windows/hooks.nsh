; Installer hooks for the browser link's native messaging host.
;
; `ud-bridge.exe` is installed beside the app, and a browser starts it for a
; moment whenever the signed-in session changes -- whether or not the app is
; running. Tauri's own check only looks for the main program, so an install
; that lands on one of those moments finds the file open and stops at an
; Abort / Retry / Ignore box. That box is the one thing an unattended update
; cannot answer, and on uninstall the same moment leaves the file behind.
;
; The helper keeps nothing in memory -- every message is one process that
; answers from files and exits -- so ending it costs at most one push, which
; the extension sends again at the next cookie change.

!macro NSIS_HOOK_PREINSTALL
  nsis_tauri_utils::KillProcessCurrentUser "ud-bridge.exe"
  Pop $R0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsis_tauri_utils::KillProcessCurrentUser "ud-bridge.exe"
  Pop $R0
!macroend
