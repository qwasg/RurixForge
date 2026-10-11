; RurixForge NSIS hooks (included by electron-builder via nsis.include).
;
; Always install per user: no admin rights, and the install directory stays writable for
; workspace skills / store packages. This also skips the "all users or just me" page, so the
; wizard is: choose directory -> install -> finish (with "run RurixForge").
!macro customInstallMode
  StrCpy $isForceCurrentInstall "1"
!macroend
