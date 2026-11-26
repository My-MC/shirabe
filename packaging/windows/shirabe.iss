; Inno Setup 6 script for the unsigned Windows x64 installer of Shirabe Meter
; Suite.
;
; Build it with packaging/windows/build-installer.ps1, which fills in the
; defines below. Running ISCC.exe directly without /D produces a 0.0.0 build
; and is not supported.
;
; Layout:
;   %CommonProgramFiles%\VST3\Shirabe Meter Suite.vst3
;   %CommonProgramFiles%\CLAP\Shirabe Meter Suite.clap
;   %ProgramFiles%\Shirabe Meter Suite\shirabe.exe
;   Start menu shortcut and uninstaller
;
; The AppId below is the product identity used for upgrades and for the
; uninstall entry. Never change it once a build has been published: changing it
; makes Windows treat the package as a different product and leaves the old
; entry and files behind. Keep it in sync with docs/building.md.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#ifndef DistDir
  #define DistDir "..\..\dist\shirabe-windows-x64"
#endif

#ifndef OutputDir
  #define OutputDir "..\..\dist\installers"
#endif

#ifndef OutputBaseFilename
  #define OutputBaseFilename "ShirabeMeterSuite-windows-x64-setup"
#endif

#define AppName "Shirabe Meter Suite"
#define AppPublisher "The Shirabe contributors"
#define AppUrl "https://github.com/My-MC/shirabe"

[Setup]
AppId={{FDE72B4E-A801-4085-89A8-50C9A6CF8B63}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}
VersionInfoDescription={#AppName} installer
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
AllowNoIcons=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\shirabe.exe
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
; The bundle directory itself is created under VST3; recursesubdirs keeps
; Contents\x86_64-win intact.
Source: "{#DistDir}\Shirabe Meter Suite.vst3\*"; DestDir: "{commoncf}\VST3\{#AppName}.vst3"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#DistDir}\Shirabe Meter Suite.clap"; DestDir: "{commoncf}\CLAP"; Flags: ignoreversion
Source: "{#DistDir}\shirabe.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\CHANGELOG.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#DistDir}\LICENSES\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}\{#AppName}"; Filename: "{app}\shirabe.exe"; Comment: "Standalone metering application"
Name: "{autoprograms}\{#AppName}\Uninstall {#AppName}"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\shirabe.exe"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
