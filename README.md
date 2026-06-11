# Image Metadata Fixer

[Download the latest installer](https://github.com/Sevi-py/image-metadata-fixer/releases/latest)

Image Metadata Fixer repairs JPEGs that make Windows Explorer fail with
`0x88982F52` when editing file details. The affected files this was built for
contain a large embedded EXIF thumbnail inside the JPEG metadata segment. The
tool removes that embedded thumbnail, keeps the real photo pixels and normal
EXIF/GPS/details tags, and leaves the image resolution unchanged.

## Install

Download `image-metadata-fixer-setup-*.exe` from the latest release and run it.
The installer installs per-user into:

```text
%LOCALAPPDATA%\Programs\ImageMetadataFixer
```

It also:

- Adds the install folder to your user `PATH`, so the CLI is available in new
  terminals as `imagefixer`.
- Adds `Fix image metadata` to the Explorer right-click menu for image files and
  folders.
- Updates an existing install in the same location when you run a newer setup.

The installer is self-contained. A fresh Windows install does not need Rust,
Git, GitHub CLI, Visual Studio Build Tools, Inno Setup, or any other developer
prerequisites to install and use Image Metadata Fixer.

On Windows 11, the entry may be under **Show more options** after right-clicking.

## Explorer Usage

Right-click an image and choose **Fix image metadata**. The popup reports the
single-file status, such as `Fixed image metadata.` or
`Already OK. No repair needed.`

Right-click a folder to process all supported JPEGs directly inside that folder.
Subfolders are not scanned from the context menu.

## CLI Usage

Check what would change:

```powershell
imagefixer check C:\Path\To\Folder
```

Fix JPEGs directly in a folder:

```powershell
imagefixer fix C:\Path\To\Folder
```

Include one level of subfolders:

```powershell
imagefixer fix --max-depth 1 C:\Path\To\Folder
```

Keep `.bak` copies next to changed files:

```powershell
imagefixer fix --backup C:\Path\To\Folder
```

Install or refresh context-menu entries from a portable build:

```powershell
imagefixer install-context-menu
```

Remove context-menu entries:

```powershell
imagefixer uninstall-context-menu
```

## Development

Install Rust with rustup:

```powershell
winget install --id Rustlang.Rustup --exact
```

Build and test:

```powershell
cargo test
cargo build --release
```

The release build produces:

```text
target\release\image_metadata_fixer.exe
target\release\imagefixer.exe
target\release\image_metadata_fixer_context.exe
```

`imagefixer.exe` is the installed CLI alias. `image_metadata_fixer.exe` is the
Cargo-built CLI binary. `image_metadata_fixer_context.exe` is the no-console
Explorer launcher that runs the CLI hidden and shows a Windows popup summary.

To build the installer locally, install Inno Setup and run:

```powershell
iscc installer\ImageMetadataFixer.iss
```

## Releases

GitHub Actions builds a new private release on functional changes to `main`.
Documentation-only, sample-only, and generated-output changes are ignored by the
release workflow.
