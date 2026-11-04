; Inno Setup script for auraFX. Built by scripts/package/windows.ps1:
;   ISCC.exe /DAppVersion=<v> /DSourceDir=<staged folder> /DOutputDir=<dist> aurafx.iss
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{2A6B9895-066C-4360-A645-978215AAFD6F}
AppName=auraFX
AppVersion={#AppVersion}
AppPublisher=auraFX contributors
AppPublisherURL=https://github.com/aashiquear/auraFX
DefaultDirName={localappdata}\Programs\auraFX
DefaultGroupName=auraFX
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=auraFX-{#AppVersion}-windows-x64-setup
SetupIconFile={#SourceDir}\aurafx.ico
UninstallDisplayIcon={app}\aurafx.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "assoc"; Description: "Open .afx session files with auraFX"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\auraFX"; Filename: "{app}\aurafx.exe"
Name: "{group}\Uninstall auraFX"; Filename: "{uninstallexe}"
Name: "{autodesktop}\auraFX"; Filename: "{app}\aurafx.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.afx"; ValueType: string; ValueName: ""; ValueData: "auraFX.Session"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\auraFX.Session\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\aurafx.exe,0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\auraFX.Session\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\aurafx.exe"" ""%1"""; Tasks: assoc

[Run]
Filename: "{app}\aurafx.exe"; Description: "Start auraFX"; Flags: nowait postinstall skipifsilent
