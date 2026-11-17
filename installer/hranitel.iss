; Установщик «Хранителя» (Inno Setup 6).
;
; Собирается скриптом scripts\build-installer.ps1:
;   iscc /DMyAppVersion=0.12.1 installer\hranitel.iss
;
; Что делает установщик:
;  - ставит приложение в %LOCALAPPDATA%\Hranitel\app (без прав администратора);
;  - ярлык в меню «Пуск», по желанию — на рабочем столе;
;  - по галочке включает автозапуск при входе в Windows (ключ Run, старт в трее);
;  - кладёт рядом расширение и скрипт установки моста к браузеру;
;  - при удалении убирает программу и ключ автозапуска, но НЕ трогает
;    хранилище и настройки в %APPDATA%\Hranitel.

#define MyAppName "Хранитель"
#define MyAppExeName "vault-win.exe"
#define MyAppPublisher "reznikov73"

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

#define MySourceDir "..\dist\Hranitel-" + MyAppVersion + "-win64"

[Setup]
AppId={{6E2B7A54-9C41-4E7B-9E2A-1B7F0C3D5A21}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Hranitel\app
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Hranitel-{#MyAppVersion}-setup
SetupIconFile=hranitel.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=no
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "autostart"; Description: "Запускать «Хранитель» при входе в Windows (свёрнутым в трей)"; GroupDescription: "Дополнительно:"; Flags: checkedonce
Name: "desktopicon"; Description: "Ярлык на рабочем столе"; GroupDescription: "Дополнительно:"; Flags: unchecked

[Files]
Source: "{#MySourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{group}\Удалить {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Hranitel"; ValueData: """{app}\{#MyAppExeName}"" --tray --quiet"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Запустить «Хранитель»"; Flags: nowait postinstall skipifsilent
