; RurixForge 的 NSIS 钩子(electron-builder 经 nsis.include 引入,早于其模板主体)。
; 本文件含中文,必须保存为带 BOM 的 UTF-8。不用 LogicLib:引入时它可能还没加载。

; 一律按当前用户安装:不需要管理员权限,安装目录保持可写;同时跳过「所有用户 / 仅我」页,
; 向导只剩:选择目录 → 安装 → 完成(可勾选立即运行)。
!macro customInstallMode
  StrCpy $isForceCurrentInstall "1"
!macroend

; electron-builder 卸载和覆盖升级时会删除整个安装目录(RMDir /r $INSTDIR)。所以只允许装进
; 新文件夹、空文件夹或已有的 RurixForge 安装目录:交互安装时「安装」按钮置灰,静默安装直接失败,
; 避免误删用户自己的文件(例如把源码仓库所在的 D:\RurixForge 选成了安装位置)。
; 目录页的说明必须在模板声明目录页之前定义,所以放在文件顶层。
!ifndef BUILD_UNINSTALLER
  !define MUI_DIRECTORYPAGE_TEXT_TOP "RurixForge 将安装到下面的文件夹。要换位置，点「浏览」选择其他磁盘或文件夹（路径里没有 RurixForge 时会自动新建 RurixForge 文件夹），再点「安装」。$\r$\n$\r$\n卸载或升级时会删除整个安装文件夹，所以不能装进已经有其他文件的文件夹（例如源码仓库），这时「安装」按钮会变灰。"
!endif

; 检查函数要用 common.nsh 里定义的 APP_EXECUTABLE_FILENAME;模板在引入 common.nsh 之后才插入 customHeader。
!macro customHeader
  !ifndef BUILD_UNINSTALLER
    Var forgeDirCheck

    ; 入栈一个目录,出栈 "ok"(不存在、为空或已是 RurixForge 安装目录)或 "foreign"。
    Function forgeCheckInstallDir
      Exch $R0
      Push $R1
      Push $R2
      StrCpy $forgeDirCheck "ok"
      IfFileExists "$R0\${APP_EXECUTABLE_FILENAME}" forgeCheckDone
      ClearErrors
      FindFirst $R1 $R2 "$R0\*.*"
      forgeCheckLoop:
        StrCmp $R2 "" forgeCheckClose
        StrCmp $R2 "." forgeCheckNext
        StrCmp $R2 ".." forgeCheckNext
        StrCpy $forgeDirCheck "foreign"
        Goto forgeCheckClose
      forgeCheckNext:
        FindNext $R1 $R2
        Goto forgeCheckLoop
      forgeCheckClose:
        FindClose $R1
      forgeCheckDone:
      Pop $R2
      Pop $R1
      Pop $R0
      Push $forgeDirCheck
    FunctionEnd

    ; 目录页每次改路径都会调用;Abort = 路径无效(「安装」按钮置灰)。
    Function .onVerifyInstDir
      Push $R0
      Push $R1
      Push $R2
      Push $R3
      ; 与模板里的 instFilesPre 一致:路径里不含应用名(不区分大小写)时,实际装到 <所选目录>\<应用名>。
      StrCpy $R0 "$INSTDIR\${APP_FILENAME}"
      StrLen $R1 "${APP_FILENAME}"
      StrCpy $R2 0
      forgeVerifyFind:
        StrCpy $R3 $INSTDIR $R1 $R2
        StrCmp $R3 "" forgeVerifyCheck
        StrCmp $R3 "${APP_FILENAME}" forgeVerifyAsIs
        IntOp $R2 $R2 + 1
        Goto forgeVerifyFind
      forgeVerifyAsIs:
        StrCpy $R0 $INSTDIR
      forgeVerifyCheck:
      Push $R0
      Call forgeCheckInstallDir
      Pop $R3
      StrCmp $R3 "ok" forgeVerifyValid
      Pop $R3
      Pop $R2
      Pop $R1
      Pop $R0
      Abort
      forgeVerifyValid:
      Pop $R3
      Pop $R2
      Pop $R1
      Pop $R0
    FunctionEnd
  !endif
!macroend

; 静默安装(/S,可带 /D=目录)不经过目录页,在初始化时检查;不安全就以退出码 2 结束,不写任何文件。
!macro customInit
  IfSilent 0 forgeInitDone
    Push $INSTDIR
    Call forgeCheckInstallDir
    Pop $forgeDirCheck
    StrCmp $forgeDirCheck "ok" forgeInitDone
    SetErrorLevel 2
    Quit
  forgeInitDone:
!macroend

; 安装界面(tools/installer/setup)的快捷方式选项:不勾选时传 --no-desktop-shortcut / --no-start-menu-shortcut。
; 模板只认前者,而且升级时旧版卸载程序会保留已有快捷方式,所以这里按本次的选择删掉不要的。
!macro customInstall
  ${StdUtils.TestParameter} $R9 "no-desktop-shortcut"
  StrCmp $R9 "true" 0 forgeDesktopDone
    IfFileExists "$newDesktopLink" 0 forgeDesktopDone
    WinShell::UninstShortcut "$newDesktopLink"
    Delete "$newDesktopLink"
    System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, i 0, i 0)'
  forgeDesktopDone:
  ${StdUtils.TestParameter} $R9 "no-start-menu-shortcut"
  StrCmp $R9 "true" 0 forgeStartMenuDone
    IfFileExists "$newStartMenuLink" 0 forgeStartMenuDone
    WinShell::UninstShortcut "$newStartMenuLink"
    Delete "$newStartMenuLink"
    StrCpy $launchLink "$appExe"
  forgeStartMenuDone:

  ; 模板会把安装包整个复制到 %LOCALAPPDATA%\<应用名>-updater\installer.exe,供 electron-updater 做差分更新。
  ; RurixForge 没有自动更新,这份和安装包一样大的副本没有用处,装完即删。
  !ifdef APP_INSTALLER_STORE_FILE
    Delete "$LOCALAPPDATA\${APP_INSTALLER_STORE_FILE}"
    ${StdUtils.GetParentPath} $R9 "$LOCALAPPDATA\${APP_INSTALLER_STORE_FILE}"
    RMDir "$R9"
  !endif
!macroend
