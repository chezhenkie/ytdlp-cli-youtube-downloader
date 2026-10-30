; ytdlp-cli windows installer (Inno Setup 6).
;
; Built by bundles.yml from the staged bundle tree (the same bytes the
; windows zip carries), so the installed product needs nothing else on the
; machine: no runtime, no redistributable. Per-user install (no admin
; prompt): {localappdata}\Programs\ytdlp-cli, the user's PATH gets the
; directory, and a real uninstaller is registered in Windows Settings.
;
; ISCC invocation (bundles.yml): ISCC /DAppVer=<version> /DStage=<staged tree> /O<dist>
; The define is AppVer, not Ver: ISPP symbols are case-insensitive and Ver is a
; builtin (the Inno Setup version), so /DVer clobbered it and aborted the
; compile inside ISPPBuiltins.iss - "Operator not applicable to this operand
; type" at its VER comparison (bundles run 39055315895, windows job).
; The user PATH entry is written from [Code], not [Registry]: a Check parameter
; on a [Registry] line arrives unexpanded, so a guard built on '{app}' compared
; the literal text, never matched, and appended the directory on every install.
; The code path expands the directory itself, normalizes quotes, trailing
; slashes and case on both sides, and rewrites the value without duplicates:
; a reinstall keeps one entry, existing duplicates collapse to one, and the
; uninstall removes every form of the entry.

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

[UninstallDelete]
; the tree is the app's own data folder too (settings.json, updated yt-dlp);
; removing the directory removes what the app wrote
Type: filesandordirs; Name: "{app}"

[Code]
function NormalizeEntry(Entry: string): string;
var
  S: string;
  N: Integer;
begin
  S := Trim(Entry);
  N := Length(S);
  if (N >= 2) and (S[1] = '"') and (S[N] = '"') then
  begin
    S := Copy(S, 2, N - 2);
    N := Length(S);
  end;
  while (N > 3) and (S[N] = '\') do
  begin
    S := Copy(S, 1, N - 1);
    N := Length(S);
  end;
  Result := Uppercase(S);
end;

function WithoutEntry(Path, Entry: string): string;
var
  Rest, Item, Target: string;
  P: Integer;
begin
  Target := NormalizeEntry(Entry);
  Rest := Path;
  Result := '';
  while Length(Rest) > 0 do
  begin
    P := Pos(';', Rest);
    if P > 0 then
    begin
      Item := Copy(Rest, 1, P - 1);
      Rest := Copy(Rest, P + 1, Length(Rest) - P);
    end
    else
    begin
      Item := Rest;
      Rest := '';
    end;
    if (Trim(Item) <> '') and (NormalizeEntry(Item) <> Target) then
    begin
      if Result = '' then
        Result := Item
      else
        Result := Result + ';' + Item;
    end;
  end;
end;

procedure WritePathEntry;
var
  OrigPath, NewPath, Entry: string;
begin
  Entry := ExpandConstant('{app}');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', OrigPath) then
    OrigPath := '';
  NewPath := WithoutEntry(OrigPath, Entry);
  if NewPath = '' then
    NewPath := Entry
  else
    NewPath := NewPath + ';' + Entry;
  if NewPath <> OrigPath then
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', NewPath);
end;

procedure RemovePathEntry;
var
  OrigPath, NewPath: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', OrigPath) then
    exit;
  NewPath := WithoutEntry(OrigPath, ExpandConstant('{app}'));
  if NewPath <> OrigPath then
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', NewPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    WritePathEntry;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemovePathEntry;
end;
