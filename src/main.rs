use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use walkdir::WalkDir;

const APP1: u8 = 0xE1;
const SOS: u8 = 0xDA;
const SOI: [u8; 2] = [0xFF, 0xD8];
const EOI: [u8; 2] = [0xFF, 0xD9];
const EXIF_PREFIX: &[u8] = b"Exif\0\0";
const JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
const JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;

#[derive(Parser)]
#[command(name = "image-metadata-fixer")]
#[command(
    about = "Losslessly repairs JPEG EXIF segment lengths that break Windows metadata editing."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fix JPEG files or all JPEGs inside folders.
    Fix(ProcessArgs),
    /// Show which files would be fixed without writing changes.
    Check(ProcessArgs),
    /// Add Explorer right-click entries for image files and folders.
    InstallContextMenu,
    /// Remove the Explorer right-click entries.
    UninstallContextMenu,
}

#[derive(Parser, Clone)]
struct ProcessArgs {
    /// Files or folders to scan.
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Folder depth to scan. 0 means only files directly in the selected folder.
    #[arg(long, default_value_t = 0)]
    max_depth: usize,

    /// Keep a .bak copy next to each fixed file.
    #[arg(long)]
    backup: bool,

    /// Wait for Enter before exiting. Used by the Explorer context menu.
    #[arg(long, hide = true)]
    pause: bool,

    /// Print only the final summary. Used by the Explorer context-menu popup.
    #[arg(long, hide = true)]
    summary_only: bool,
}

#[derive(Default)]
struct Summary {
    fixed: usize,
    would_fix: usize,
    already_ok: usize,
    unsupported: usize,
    failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairStatus {
    NeedsRepair {
        old_len: u16,
        new_len: u16,
        removed_thumbnail_bytes: usize,
    },
    AlreadyOk,
    Unsupported,
}

#[derive(Debug, Clone, Copy)]
struct ExifThumbnailInfo {
    app1_offset: usize,
    old_app1_len: u16,
    new_app1_len: u16,
    app1_end: usize,
    ifd0_next_offset_pos: usize,
    ifd1_start: usize,
    thumbnail_start: usize,
    thumbnail_end: usize,
    big_endian: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Fix(args) => {
            let result = process_paths(&args, false);
            if args.pause {
                pause();
            }
            result
        }
        Command::Check(args) => {
            let result = process_paths(&args, true);
            if args.pause {
                pause();
            }
            result
        }
        Command::InstallContextMenu => install_context_menu(),
        Command::UninstallContextMenu => uninstall_context_menu(),
    }
}

fn process_paths(args: &ProcessArgs, dry_run: bool) -> Result<()> {
    let mut summary = Summary::default();

    for path in expand_inputs(&args.paths, args.max_depth)? {
        if !looks_like_jpeg(&path) {
            summary.unsupported += 1;
            if !args.summary_only {
                println!("unsupported: {}", path.display());
            }
            continue;
        }

        match process_file(&path, dry_run, args.backup) {
            Ok(RepairStatus::NeedsRepair {
                old_len,
                new_len,
                removed_thumbnail_bytes,
            }) if dry_run => {
                summary.would_fix += 1;
                if !args.summary_only {
                    println!(
                        "would fix: {} (APP1 length {} -> {}, remove {} thumbnail bytes)",
                        path.display(),
                        old_len,
                        new_len,
                        removed_thumbnail_bytes
                    );
                }
            }
            Ok(RepairStatus::NeedsRepair {
                old_len,
                new_len,
                removed_thumbnail_bytes,
            }) => {
                summary.fixed += 1;
                if !args.summary_only {
                    println!(
                        "fixed: {} (APP1 length {} -> {}, removed {} thumbnail bytes)",
                        path.display(),
                        old_len,
                        new_len,
                        removed_thumbnail_bytes
                    );
                }
            }
            Ok(RepairStatus::AlreadyOk) => {
                summary.already_ok += 1;
                if !args.summary_only {
                    println!("already ok: {}", path.display());
                }
            }
            Ok(RepairStatus::Unsupported) => {
                summary.unsupported += 1;
                if !args.summary_only {
                    println!("unsupported: {}", path.display());
                }
            }
            Err(err) => {
                summary.failed += 1;
                if !args.summary_only {
                    eprintln!("failed: {}: {err:#}", path.display());
                }
            }
        }
    }

    if dry_run {
        println!(
            "summary: {} would fix, {} already ok, {} unsupported, {} failed",
            summary.would_fix, summary.already_ok, summary.unsupported, summary.failed
        );
    } else {
        println!(
            "summary: {} fixed, {} already ok, {} unsupported, {} failed",
            summary.fixed, summary.already_ok, summary.unsupported, summary.failed
        );
    }

    if summary.failed == 0 {
        Ok(())
    } else {
        bail!("one or more files failed")
    }
}

fn expand_inputs(paths: &[PathBuf], max_depth: usize) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        let metadata = fs::metadata(path)
            .with_context(|| format!("cannot read metadata for {}", path.display()))?;

        if metadata.is_file() {
            files.push(path.clone());
            continue;
        }

        if metadata.is_dir() {
            for entry in WalkDir::new(path)
                .follow_links(false)
                .min_depth(1)
                .max_depth(max_depth + 1)
            {
                let entry = entry.with_context(|| format!("cannot scan {}", path.display()))?;
                if entry.file_type().is_file() {
                    files.push(entry.into_path());
                }
            }
            continue;
        }

        println!("skipped: {}", path.display());
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn looks_like_jpeg(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "jpe"))
        .unwrap_or(false)
}

fn process_file(path: &Path, dry_run: bool, backup: bool) -> Result<RepairStatus> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let (status, repaired_bytes) = repair_jpeg_bytes(&bytes)?;

    if dry_run {
        return Ok(status);
    }

    let Some(repaired_bytes) = repaired_bytes else {
        return Ok(status);
    };

    if backup {
        let backup_path = next_backup_path(path);
        fs::copy(path, &backup_path).with_context(|| {
            format!(
                "cannot create backup {} for {}",
                backup_path.display(),
                path.display()
            )
        })?;
    }

    fs::write(path, repaired_bytes).with_context(|| format!("cannot write {}", path.display()))?;

    Ok(status)
}

fn next_backup_path(path: &Path) -> PathBuf {
    let mut candidate = path.with_extension(format!(
        "{}.bak",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg")
    ));
    let mut index = 1;

    while candidate.exists() {
        candidate = path.with_extension(format!(
            "{}.bak{}",
            path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("jpg"),
            index
        ));
        index += 1;
    }

    candidate
}

fn analyze_jpeg(bytes: &[u8]) -> Result<RepairStatus> {
    Ok(match find_thumbnail_repair(bytes)? {
        Some(info) => repair_status(info),
        None => {
            if bytes.len() < 4 || bytes[0..2] != SOI {
                RepairStatus::Unsupported
            } else {
                RepairStatus::AlreadyOk
            }
        }
    })
}

fn repair_jpeg_bytes(bytes: &[u8]) -> Result<(RepairStatus, Option<Vec<u8>>)> {
    let Some(info) = find_thumbnail_repair(bytes)? else {
        return Ok((analyze_jpeg(bytes)?, None));
    };

    let mut repaired = Vec::with_capacity(bytes.len() - (info.app1_end - info.ifd1_start));
    repaired.extend_from_slice(&bytes[..info.app1_offset + 2]);
    repaired.extend_from_slice(&info.new_app1_len.to_be_bytes());
    repaired.extend_from_slice(&bytes[info.app1_offset + 4..info.ifd0_next_offset_pos]);

    if info.big_endian {
        repaired.extend_from_slice(&0u32.to_be_bytes());
    } else {
        repaired.extend_from_slice(&0u32.to_le_bytes());
    }

    repaired.extend_from_slice(&bytes[info.ifd0_next_offset_pos + 4..info.ifd1_start]);
    repaired.extend_from_slice(&bytes[info.app1_end..]);

    Ok((repair_status(info), Some(repaired)))
}

fn repair_status(info: ExifThumbnailInfo) -> RepairStatus {
    RepairStatus::NeedsRepair {
        old_len: info.old_app1_len,
        new_len: info.new_app1_len,
        removed_thumbnail_bytes: info.thumbnail_end - info.thumbnail_start,
    }
}

fn find_thumbnail_repair(bytes: &[u8]) -> Result<Option<ExifThumbnailInfo>> {
    if bytes.len() < 4 || bytes[0..2] != SOI {
        return Ok(None);
    }

    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        let marker_pos = pos;
        if bytes[pos] != 0xFF {
            return Ok(None);
        }

        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }

        if pos >= bytes.len() {
            return Ok(None);
        }

        let marker = bytes[pos];
        pos += 1;

        if marker == SOS {
            return Ok(None);
        }

        if is_standalone_marker(marker) {
            continue;
        }

        if pos + 2 > bytes.len() {
            return Ok(None);
        }

        let declared_len = read_be_u16(bytes, pos)? as usize;
        if declared_len < 2 {
            return Ok(None);
        }

        let payload_start = pos + 2;
        let declared_end = pos + declared_len;
        if declared_end > bytes.len() {
            return Ok(None);
        }

        if marker == APP1 && bytes[payload_start..declared_end].starts_with(EXIF_PREFIX) {
            if let Some(mut info) =
                exif_thumbnail_info(bytes, marker_pos, payload_start, declared_len as u16)
            {
                let fixed_app1_end = marker_pos + 2 + info.old_app1_len as usize;
                let app1_end = if info.thumbnail_end > fixed_app1_end {
                    info.thumbnail_end
                } else {
                    fixed_app1_end
                };

                if next_outer_marker_is_plausible(bytes, app1_end) {
                    info.app1_end = app1_end;
                    return Ok(Some(info));
                }
            }
        }

        pos = declared_end;
    }

    Ok(None)
}

fn exif_thumbnail_info(
    bytes: &[u8],
    app1_offset: usize,
    app1_payload_start: usize,
    old_app1_len: u16,
) -> Option<ExifThumbnailInfo> {
    let tiff_start = app1_payload_start.checked_add(EXIF_PREFIX.len())?;
    let endian = bytes.get(tiff_start..tiff_start + 2)?;
    let big_endian = match endian {
        b"II" => false,
        b"MM" => true,
        _ => return None,
    };

    if read_u16(bytes, tiff_start + 2, big_endian).ok()? != 42 {
        return None;
    }

    let ifd0_offset = read_u32(bytes, tiff_start + 4, big_endian).ok()? as usize;
    let ifd0 = tiff_start.checked_add(ifd0_offset)?;
    let (ifd1_offset, ifd0_next_offset_pos) = next_ifd_offset(bytes, tiff_start, ifd0, big_endian)?;
    if ifd1_offset == 0 {
        return None;
    }

    let ifd1_start = tiff_start.checked_add(ifd1_offset as usize)?;
    let (thumbnail_offset, thumbnail_len) = jpeg_thumbnail_tags(bytes, ifd1_start, big_endian)?;
    if thumbnail_offset == 0 || thumbnail_len < 4 {
        return None;
    }

    let thumbnail_start = tiff_start.checked_add(thumbnail_offset as usize)?;
    let thumbnail_end = thumbnail_start.checked_add(thumbnail_len as usize)?;
    if thumbnail_end > bytes.len() {
        return None;
    }

    if bytes.get(thumbnail_start..thumbnail_start + 2)? != SOI {
        return None;
    }

    if bytes.get(thumbnail_end - 2..thumbnail_end)? != EOI {
        return None;
    }

    let new_app1_len = ifd1_start
        .checked_sub(app1_offset + 2)
        .and_then(|len| u16::try_from(len).ok())
        .filter(|len| *len >= 2)?;

    Some(ExifThumbnailInfo {
        app1_offset,
        old_app1_len,
        new_app1_len,
        app1_end: 0,
        ifd0_next_offset_pos,
        ifd1_start,
        thumbnail_start,
        thumbnail_end,
        big_endian,
    })
}

fn next_ifd_offset(
    bytes: &[u8],
    tiff_start: usize,
    ifd_offset: usize,
    big_endian: bool,
) -> Option<(u32, usize)> {
    let entry_count = read_u16(bytes, ifd_offset, big_endian).ok()? as usize;
    let entries_start = ifd_offset.checked_add(2)?;
    let next_offset_pos = entries_start.checked_add(entry_count.checked_mul(12)?)?;
    let next_offset = read_u32(bytes, next_offset_pos, big_endian).ok()?;

    if next_offset == 0 {
        return Some((0, next_offset_pos));
    }

    let next_absolute = tiff_start.checked_add(next_offset as usize)?;
    if next_absolute + 2 <= bytes.len() {
        Some((next_offset, next_offset_pos))
    } else {
        None
    }
}

fn jpeg_thumbnail_tags(bytes: &[u8], ifd_offset: usize, big_endian: bool) -> Option<(u32, u32)> {
    let entry_count = read_u16(bytes, ifd_offset, big_endian).ok()? as usize;
    let entries_start = ifd_offset.checked_add(2)?;
    let mut thumbnail_offset = None;
    let mut thumbnail_len = None;

    for index in 0..entry_count {
        let entry = entries_start.checked_add(index.checked_mul(12)?)?;
        if entry + 12 > bytes.len() {
            return None;
        }

        let tag = read_u16(bytes, entry, big_endian).ok()?;
        let value = read_u32(bytes, entry + 8, big_endian).ok()?;

        match tag {
            JPEG_INTERCHANGE_FORMAT => thumbnail_offset = Some(value),
            JPEG_INTERCHANGE_FORMAT_LENGTH => thumbnail_len = Some(value),
            _ => {}
        }
    }

    Some((thumbnail_offset?, thumbnail_len?))
}

fn next_outer_marker_is_plausible(bytes: &[u8], pos: usize) -> bool {
    if pos >= bytes.len() {
        return true;
    }

    bytes.get(pos) == Some(&0xFF)
        && bytes
            .get(pos + 1)
            .map(|marker| *marker != 0x00 && *marker != 0xFF)
            .unwrap_or(false)
}

fn is_standalone_marker(marker: u8) -> bool {
    marker == 0x01 || (0xD0..=0xD9).contains(&marker)
}

fn read_be_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .context("unexpected end while reading u16")?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn read_u16(bytes: &[u8], offset: usize, big_endian: bool) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .context("unexpected end while reading TIFF u16")?;

    Ok(if big_endian {
        u16::from_be_bytes([value[0], value[1]])
    } else {
        u16::from_le_bytes([value[0], value[1]])
    })
}

fn read_u32(bytes: &[u8], offset: usize, big_endian: bool) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .context("unexpected end while reading TIFF u32")?;

    Ok(if big_endian {
        u32::from_be_bytes([value[0], value[1], value[2], value[3]])
    } else {
        u32::from_le_bytes([value[0], value[1], value[2], value[3]])
    })
}

fn pause() {
    println!("Press Enter to close...");
    let _ = std::io::stdin().read(&mut [0u8]).ok();
}

#[cfg(windows)]
fn install_context_menu() -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let exe = std::env::current_exe().context("cannot determine current executable path")?;
    let mut context_exe = exe.clone();
    context_exe.set_file_name("image_metadata_fixer_context.exe");
    let launcher = if context_exe.exists() {
        context_exe
    } else {
        exe.clone()
    };
    let command = if launcher.file_name().and_then(|name| name.to_str())
        == Some("image_metadata_fixer_context.exe")
    {
        format!("\"{}\" \"%1\"", launcher.display())
    } else {
        format!(
            "\"{}\" fix --max-depth 0 --summary-only \"%1\"",
            launcher.display()
        )
    };
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    for key_path in legacy_context_menu_keys() {
        let _ = hkcu.delete_subkey_all(key_path);
    }

    for key_path in context_menu_keys() {
        let (key, _) = hkcu.create_subkey(key_path)?;
        key.set_value("", &"Fix image metadata")?;
        key.set_value("MUIVerb", &"Fix image metadata")?;
        key.set_value("Icon", &launcher.display().to_string())?;
        key.set_value("MultiSelectModel", &"Player")?;

        let (command_key, _) = key.create_subkey("command")?;
        command_key.set_value("", &command)?;
    }

    println!("Installed Explorer context menu entries for JPEG files and folders.");
    Ok(())
}

#[cfg(not(windows))]
fn install_context_menu() -> Result<()> {
    bail!("context menu installation is only supported on Windows")
}

#[cfg(windows)]
fn uninstall_context_menu() -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key_path in context_menu_keys()
        .iter()
        .chain(legacy_context_menu_keys().iter())
    {
        let _ = hkcu.delete_subkey_all(key_path);
    }

    println!("Removed Explorer context menu entries.");
    Ok(())
}

#[cfg(not(windows))]
fn uninstall_context_menu() -> Result<()> {
    bail!("context menu removal is only supported on Windows")
}

#[cfg(windows)]
fn context_menu_keys() -> [&'static str; 4] {
    [
        r"Software\Classes\Directory\shell\ImageMetadataFixer",
        r"Software\Classes\SystemFileAssociations\.jpg\shell\ImageMetadataFixer",
        r"Software\Classes\SystemFileAssociations\.jpeg\shell\ImageMetadataFixer",
        r"Software\Classes\SystemFileAssociations\.jpe\shell\ImageMetadataFixer",
    ]
}

#[cfg(windows)]
fn legacy_context_menu_keys() -> [&'static str; 2] {
    [
        r"Software\Classes\SystemFileAssociations\image\shell\ImageMetadataFixer",
        r"Software\Classes\AllFilesystemObjects\shell\ImageMetadataFixer",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_jpeg() {
        let status = analyze_jpeg(b"not a jpeg").unwrap();
        assert_eq!(status, RepairStatus::Unsupported);
    }

    #[test]
    fn detects_generated_exif_thumbnail_fixture() {
        let bytes = generated_thumbnail_fixture();
        let status = analyze_jpeg(&bytes).unwrap();
        assert_eq!(
            status,
            RepairStatus::NeedsRepair {
                old_len: 28,
                new_len: 22,
                removed_thumbnail_bytes: 4,
            }
        );
    }

    #[test]
    fn removes_generated_exif_thumbnail_fixture() {
        let bytes = generated_thumbnail_fixture();
        let (status, repaired) = repair_jpeg_bytes(&bytes).unwrap();
        assert!(matches!(status, RepairStatus::NeedsRepair { .. }));

        let repaired = repaired.unwrap();
        assert_eq!(read_be_u16(&repaired, 4).unwrap(), 22);
        assert_eq!(analyze_jpeg(&repaired).unwrap(), RepairStatus::AlreadyOk);
    }

    fn generated_thumbnail_fixture() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE1];
        bytes.extend_from_slice(&28u16.to_be_bytes());
        bytes.extend_from_slice(EXIF_PREFIX);

        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());

        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&14u32.to_le_bytes());

        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&JPEG_INTERCHANGE_FORMAT.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&44u32.to_le_bytes());
        bytes.extend_from_slice(&JPEG_INTERCHANGE_FORMAT_LENGTH.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());

        bytes.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        bytes.extend_from_slice(&[0xFF, 0xE2, 0x00, 0x02]);
        bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        bytes
    }
}
