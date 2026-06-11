#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif

#define MyAppName "Image Metadata Fixer"
#define MyAppPublisher "Image Metadata Fixer"
#define MyAppExeName "image_metadata_fixer.exe"
#define MyAppCliAliasName "imagefixer.exe"
#define MyAppContextExeName "image_metadata_fixer_context.exe"

[Setup]
AppId={{F13C4995-8EAE-4E45-8E3E-9AF7AE7C4E74}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\ImageMetadataFixer
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=image-metadata-fixer-setup-{#MyAppVersion}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#MyAppCliAliasName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#MyAppContextExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Image Metadata Fixer"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall Image Metadata Fixer"; Filename: "{uninstallexe}"

[Registry]
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ImageMetadataFixer"; ValueType: string; ValueName: ""; ValueData: "Fix image metadata"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ImageMetadataFixer"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Fix image metadata"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ImageMetadataFixer"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#MyAppContextExeName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ImageMetadataFixer\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppContextExeName}"" ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\image\shell\ImageMetadataFixer"; ValueType: string; ValueName: ""; ValueData: "Fix image metadata"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\image\shell\ImageMetadataFixer"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Fix image metadata"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\image\shell\ImageMetadataFixer"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#MyAppContextExeName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\image\shell\ImageMetadataFixer\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppContextExeName}"" ""%1"""; Flags: uninsdeletekey

[Code]
const
  EnvironmentKey = 'Environment';

function NormalizeDir(Value: string): string;
begin
  Result := RemoveBackslash(ExpandConstant(Value));
end;

function PathContainsDir(PathValue: string; Dir: string): Boolean;
var
  Part: string;
  Remaining: string;
  Separator: Integer;
begin
  Result := False;
  Dir := Lowercase(NormalizeDir(Dir));
  Remaining := PathValue;

  while Remaining <> '' do
  begin
    Separator := Pos(';', Remaining);
    if Separator > 0 then
    begin
      Part := Copy(Remaining, 1, Separator - 1);
      Remaining := Copy(Remaining, Separator + 1, MaxInt);
    end
    else
    begin
      Part := Remaining;
      Remaining := '';
    end;

    if Lowercase(NormalizeDir(Part)) = Dir then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

procedure AddInstallDirToUserPath;
var
  PathValue: string;
  AppDir: string;
begin
  AppDir := ExpandConstant('{app}');
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', PathValue) then
    PathValue := '';

  if not PathContainsDir(PathValue, AppDir) then
  begin
    if (PathValue <> '') and (Copy(PathValue, Length(PathValue), 1) <> ';') then
      PathValue := PathValue + ';';

    PathValue := PathValue + AppDir;
    RegWriteStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', PathValue);
  end;
end;

procedure RemoveInstallDirFromUserPath;
var
  PathValue: string;
  NewPath: string;
  AppDir: string;
  Part: string;
  Remaining: string;
  Separator: Integer;
begin
  AppDir := Lowercase(NormalizeDir(ExpandConstant('{app}')));
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', PathValue) then
    Exit;

  Remaining := PathValue;
  NewPath := '';

  while Remaining <> '' do
  begin
    Separator := Pos(';', Remaining);
    if Separator > 0 then
    begin
      Part := Copy(Remaining, 1, Separator - 1);
      Remaining := Copy(Remaining, Separator + 1, MaxInt);
    end
    else
    begin
      Part := Remaining;
      Remaining := '';
    end;

    if (Part <> '') and (Lowercase(NormalizeDir(Part)) <> AppDir) then
    begin
      if NewPath <> '' then
        NewPath := NewPath + ';';
      NewPath := NewPath + Part;
    end;
  end;

  RegWriteStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', NewPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddInstallDirToUserPath;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveInstallDirFromUserPath;
end;
