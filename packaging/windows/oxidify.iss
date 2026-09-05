; The Windows installer, built with Inno Setup 6.3 or later from a release
; binary (the release workflow does this on every tag):
;
;   iscc /DVersion=0.3.0 /DArch=x86_64 /DBinary=...\oxidify.exe ^
;        /DOutputDir=dist packaging\windows\oxidify.iss
;
; Arch is x86_64 or aarch64, as in the Rust target triple, so the installer
; is named like the zip next to it. It needs no administrator rights: the
; program goes to the user's own Programs folder with a Start menu entry,
; and a running copy is closed before an update replaces it.

#ifndef Version
  #error Version must be defined on the ISCC command line
#endif
#ifndef Arch
  #error Arch must be defined on the ISCC command line (x86_64 or aarch64)
#endif
#ifndef Binary
  #error Binary must be defined on the ISCC command line
#endif
#ifndef OutputDir
  #error OutputDir must be defined on the ISCC command line
#endif
#if Arch == "aarch64"
  #define InnoArch "arm64"
#else
  #define InnoArch "x64compatible"
#endif

#define AppName "Oxidify"
#define AppExeName "oxidify.exe"

[Setup]
; Never change: this is how Windows tells an update from a new program.
AppId={{7E9C4B61-3A28-4E75-9C0D-6F1B2A48D915}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=Master0fFate
AppPublisherURL=https://github.com/Master0fFate/oxidify
AppSupportURL=https://github.com/Master0fFate/oxidify/issues
AppUpdatesURL=https://github.com/Master0fFate/oxidify/releases
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#InnoArch}
ArchitecturesInstallIn64BitMode={#InnoArch}
MinVersion=10.0
LicenseFile=..\..\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=oxidify-v{#Version}-{#Arch}-pc-windows-msvc-setup
SetupIconFile=oxidify.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; First ask recent releases to run their orderly shutdown. `force` remains
; necessary for older releases, which interpret the close request as
; close-to-tray and therefore keep the executable locked.
CloseApplications=force
RestartApplications=no
UninstallDisplayIcon={app}\{#AppExeName}
VersionInfoVersion={#Version}.0

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\third_party\yt-dlp\NOTICE"; DestDir: "{app}"; DestName: "yt-dlp-NOTICE.txt"; Flags: ignoreversion
Source: "..\..\third_party\yt-dlp\LICENSE"; DestDir: "{app}"; DestName: "yt-dlp-LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  { New releases acknowledge `quit` and finish their own audio teardown.
    Older releases reject it; CloseApplications=force below is the fallback. }
  if FileExists(ExpandConstant('{app}\{#AppExeName}')) then
    Exec(ExpandConstant('{app}\{#AppExeName}'), 'quit', '', SW_HIDE,
      ewWaitUntilTerminated, ResultCode);
  Result := '';
end;

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
