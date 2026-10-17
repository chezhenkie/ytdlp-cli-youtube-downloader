; ytdlp-cli windows installer (Inno Setup 6).
;
; Built by bundles.yml from the staged portable tree (the same bytes the
; portable zip carries), so the installed product needs nothing else on the
; machine: no runtime, no redistributable. Per-user install (no admin
; prompt): {localappdata}\Programs\ytdlp-cli, the user's PATH gets the
; directory, and a real uninstaller is registered in Windows Settings.
;
; ISCC invocation (bundles.yml): ISCC /DAppVer=<version> /DStage=<staged tree> /O<dist>
; The define is AppVer, not Ver: ISPP symbols are case-insensitive and Ver is a
; builtin (the Inno Setup version), so /DVer clobbered it and aborted the
; compile inside ISPPBuiltins.iss - "Operator not applicable to this operand
; type" at its VER comparison (bundles run 39055315895, windows job).
; The [Registry] entry appends the install dir to the user's PATH once;
; the [Code] snippet keeps reinstalls clean and removes the entry again on
; uninstall.

#ifndef AppVer
  #define AppVer "0.0.0"
#endif
#ifndef Stage
  #define Stage "..\..\dist\ytdlp-cli-v0.0.0"
#endif

[Setup]
AppId={{8E4B0C21-7D3A-4E68-9A5F-2C6D9E1B7F30}
AppName=ytdlp
AppVersion={#AppVer}
AppVerName=ytdlp {#AppVer}
AppPublisher=chezhenkie
AppPublisherURL=https://github.com/chezhenkie/ytdlp-cli-youtube-downloader
DefaultDirName={localappdata}\Programs\ytdlp-cli
DisableProgramGroupPage=yes
DefaultGroupName=ytdlp
PrivilegesRequired=lowest
MinVersion=10.0
OutputDir=.
OutputBaseFilename=ytdlp-cli-v{#AppVer}-windows-setup
SetupIconFile=icon.ico
UninstallDisplayIcon={app}\ytdlp.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
InfoAfterFile=installer-after.txt

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\ytdlp"; Filename: "{app}\ytdlp.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\ytdlp"; Filename: "{app}\ytdlp.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\ytdlp.exe"; Description: "Start ytdlp now"; Flags: nowait postinstall skipifsilent unchecked

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath('{app}')

[UninstallDelete]
; the tree is the app's own data folder too (settings.json, updated yt-dlp,
; an offered ffmpeg); removing the directory removes what the app wrote
Type: filesandordirs; Name: "{app}"

[Code]
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;

procedure RemoveFromPath(Path: string);
var
  Before, OrigPath: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', OrigPath) then
    exit;
  Before := OrigPath;
  StringChangeEx(OrigPath, ';' + Path, '', True);
  StringChangeEx(OrigPath, Path + ';', '', True);
  if OrigPath = Path then
    OrigPath := '';
  if OrigPath <> Before then
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', OrigPath);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
