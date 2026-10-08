; The build script defines PayloadDir, FilesInclude, InstallerOutput and BuildDate.
[Setup]
AppId={{99D99C87-2C73-47F1-AFE2-55D02F25352C}
AppName=编译防线 V6 试玩版
AppVersion=6.0-preview.{#BuildDate}
AppVerName=编译防线 V6 试玩版 ({#BuildDate})
AppPublisher=qwasg
AppPublisherURL=https://github.com/qwasg/RurixForge
AppSupportURL=https://github.com/qwasg/RurixForge/issues
DefaultDirName={localappdata}\Programs\CodeSentinelsV6Preview
DefaultGroupName=编译防线 V6 试玩版
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible and not arm64
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
WizardStyle=modern
DisableWelcomePage=no
InfoBeforeFile={#PayloadDir}\INSTALL-README.txt
OutputDir={#InstallerOutput}
OutputBaseFilename=CodeSentinels-V6-Preview-Setup-{#BuildDate}
Compression=lzma2/fast
SolidCompression=yes
LZMADictionarySize=32768
SetupLogging=yes
UninstallDisplayName=编译防线 V6 试玩版
UninstallDisplayIcon={app}\bin\engine-host.exe
CloseApplications=yes
RestartApplications=no
DirExistsWarning=yes
VersionInfoVersion=6.0.0.0
VersionInfoProductName=Code Sentinels V6
VersionInfoDescription=Code Sentinels V6 Preview Installer

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
#ifdef ChineseMessages
Name: "chinesesimplified"; MessagesFile: "{#ChineseMessages}"
#endif

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式："

[Files]
#include FilesInclude

[Icons]
Name: "{group}\编译防线 V6 试玩版"; Filename: "{app}\Play-Game.cmd"; WorkingDir: "{app}"; Flags: runminimized
Name: "{autodesktop}\编译防线 V6 试玩版"; Filename: "{app}\Play-Game.cmd"; WorkingDir: "{app}"; Tasks: desktopicon; Flags: runminimized
Name: "{group}\试玩说明"; Filename: "{app}\INSTALL-README.txt"
Name: "{group}\卸载编译防线 V6 试玩版"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\Play-Game.cmd"; WorkingDir: "{app}"; Description: "启动编译防线 V6 试玩版"; Flags: postinstall skipifsilent shellexec nowait runminimized

; No recursive uninstall deletion: player saves/logs created after install stay.
