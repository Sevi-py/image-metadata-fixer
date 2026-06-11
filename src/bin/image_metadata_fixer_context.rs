#![windows_subsystem = "windows"]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BATCH_WAIT: Duration = Duration::from_millis(600);
const LOCK_RETRY_WAIT: Duration = Duration::from_millis(10);
const STALE_FILE_AFTER: Duration = Duration::from_secs(30);

fn main() {
    let message = match run_fixer() {
        Ok(Some(message)) => message,
        Ok(None) => return,
        Err(err) => format!("Failed to run Image Metadata Fixer:\n{err}"),
    };

    show_message(
        "Image Metadata Fixer",
        &message,
        message.starts_with("Failed"),
    );
}

fn run_fixer() -> Result<Option<String>, String> {
    let targets: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let targets = enqueue_targets(targets)?;
    let Some(targets) = targets else {
        return Ok(None);
    };

    let target_is_file = targets.len() == 1 && targets[0].is_file();

    let fixer = fixer_exe_path()?;
    let mut command = Command::new(&fixer);
    command
        .arg("fix")
        .arg("--max-depth")
        .arg("0")
        .arg("--summary-only");

    for target in &targets {
        command.arg(target);
    }

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|err| format!("Could not start {}: {err}", fixer.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let mut message = String::new();

    if !stdout.is_empty() {
        message.push_str(&stdout);
    }

    if !stderr.is_empty() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(&stderr);
    }

    if message.is_empty() {
        message.push_str(if output.status.success() {
            "Done."
        } else {
            "The fixer exited without producing a summary."
        });
    }

    Ok(Some(clean_popup_message(&message, target_is_file)))
}

fn enqueue_targets(targets: Vec<PathBuf>) -> Result<Option<Vec<PathBuf>>, String> {
    if targets.is_empty() {
        return Err("No file or folder was provided.".to_string());
    }

    let dir = queue_dir()?;
    remove_stale_file(&dir.join("leader"));
    append_targets(&dir, &targets)?;

    let leader_path = dir.join("leader");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&leader_path)
    {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(err) => return Err(format!("Could not create context-menu queue: {err}")),
    }

    thread::sleep(BATCH_WAIT);
    let queued = read_and_clear_targets(&dir, &leader_path)?;

    if queued.is_empty() {
        Ok(Some(targets))
    } else {
        Ok(Some(queued))
    }
}

fn queue_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("ImageMetadataFixerContext");
    fs::create_dir_all(&dir).map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    Ok(dir)
}

fn append_targets(dir: &Path, targets: &[PathBuf]) -> Result<(), String> {
    with_queue_lock(dir, || {
        let queue_path = dir.join("targets.bin");
        let mut queue = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&queue_path)
            .map_err(|err| format!("Could not open {}: {err}", queue_path.display()))?;

        for target in targets {
            let text = target.to_string_lossy();
            let bytes = text.as_bytes();
            let len = u32::try_from(bytes.len())
                .map_err(|_| format!("Path is too long for context-menu queue: {text}"))?;
            queue
                .write_all(&len.to_le_bytes())
                .and_then(|_| queue.write_all(bytes))
                .map_err(|err| format!("Could not write context-menu queue: {err}"))?;
        }

        Ok(())
    })
}

fn read_and_clear_targets(dir: &Path, leader_path: &Path) -> Result<Vec<PathBuf>, String> {
    with_queue_lock(dir, || {
        let queue_path = dir.join("targets.bin");
        let mut bytes = Vec::new();

        match File::open(&queue_path) {
            Ok(mut queue) => {
                queue
                    .read_to_end(&mut bytes)
                    .map_err(|err| format!("Could not read {}: {err}", queue_path.display()))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("Could not open {}: {err}", queue_path.display())),
        }

        let _ = fs::remove_file(queue_path);
        let _ = fs::remove_file(leader_path);
        Ok(decode_targets(&bytes))
    })
}

fn decode_targets(bytes: &[u8]) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let mut pos = 0;

    while pos + 4 <= bytes.len() {
        let len = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;

        if pos + len > bytes.len() {
            break;
        }

        targets.push(PathBuf::from(
            String::from_utf8_lossy(&bytes[pos..pos + len]).to_string(),
        ));
        pos += len;
    }

    targets
}

fn with_queue_lock<T>(
    dir: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock_path = dir.join("targets.lock");

    for _ in 0..200 {
        remove_stale_file(&lock_path);

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => {
                let result = operation();
                let _ = fs::remove_file(lock_path);
                return result;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                thread::sleep(LOCK_RETRY_WAIT);
            }
            Err(err) => return Err(format!("Could not lock context-menu queue: {err}")),
        }
    }

    Err("Timed out while waiting for the context-menu queue.".to_string())
}

fn remove_stale_file(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let Ok(modified) = metadata.modified() else {
        return;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return;
    };

    if age > STALE_FILE_AFTER {
        let _ = fs::remove_file(path);
    }
}

fn clean_popup_message(message: &str, target_is_file: bool) -> String {
    let cleaned = message
        .strip_prefix("summary:")
        .map(str::trim)
        .unwrap_or(message)
        .to_string();

    if target_is_file {
        single_file_status(&cleaned).unwrap_or(cleaned)
    } else {
        cleaned
    }
}

fn single_file_status(summary: &str) -> Option<String> {
    let fixed = count_before(summary, " fixed")?;
    let already_ok = count_before(summary, " already ok")?;
    let unsupported = count_before(summary, " unsupported")?;
    let failed = count_before(summary, " failed")?;

    let status = if fixed == 1 && already_ok == 0 && unsupported == 0 && failed == 0 {
        "Fixed image metadata."
    } else if fixed == 0 && already_ok == 1 && unsupported == 0 && failed == 0 {
        "Already OK. No repair needed."
    } else if fixed == 0 && already_ok == 0 && unsupported == 1 && failed == 0 {
        "Unsupported file. No changes made."
    } else if failed > 0 {
        "Failed to fix image metadata."
    } else {
        return None;
    };

    Some(status.to_string())
}

fn count_before(summary: &str, marker: &str) -> Option<usize> {
    let marker_start = summary.find(marker)?;
    let prefix = &summary[..marker_start];
    let number = prefix
        .rsplit(|ch: char| !ch.is_ascii_digit())
        .find(|part| !part.is_empty())?;

    number.parse().ok()
}

fn fixer_exe_path() -> Result<PathBuf, String> {
    let mut exe = std::env::current_exe().map_err(|err| err.to_string())?;
    exe.set_file_name("imagefixer.exe");

    if exe.exists() {
        return Ok(exe);
    }

    exe.set_file_name("image_metadata_fixer.exe");

    if exe.exists() {
        Ok(exe)
    } else {
        Err(format!("Could not find {}", exe.display()))
    }
}

#[cfg(windows)]
fn show_message(title: &str, message: &str, is_error: bool) {
    let title = wide_null(title);
    let message = wide_null(message);
    let icon = if is_error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | icon,
        );
    }
}

#[cfg(not(windows))]
fn show_message(_title: &str, message: &str, _is_error: bool) {
    eprintln!("{message}");
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_folder_style_summary_for_multi_selection() {
        let message = clean_popup_message(
            "summary: 1 fixed, 0 already ok, 2 unsupported, 0 failed",
            false,
        );

        assert_eq!(message, "1 fixed, 0 already ok, 2 unsupported, 0 failed");
    }

    #[test]
    fn keeps_single_file_friendly_status() {
        let message = clean_popup_message(
            "summary: 1 fixed, 0 already ok, 0 unsupported, 0 failed",
            true,
        );

        assert_eq!(message, "Fixed image metadata.");
    }

    #[test]
    fn decodes_length_prefixed_targets() {
        let mut bytes = Vec::new();
        for target in ["C:\\Photos\\a.jpg", "C:\\Docs\\notes.txt"] {
            bytes.extend_from_slice(&(target.len() as u32).to_le_bytes());
            bytes.extend_from_slice(target.as_bytes());
        }

        let targets = decode_targets(&bytes);

        assert_eq!(
            targets,
            vec![
                PathBuf::from("C:\\Photos\\a.jpg"),
                PathBuf::from("C:\\Docs\\notes.txt"),
            ]
        );
    }
}
