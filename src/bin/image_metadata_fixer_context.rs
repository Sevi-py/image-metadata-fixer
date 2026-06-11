#![windows_subsystem = "windows"]

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() {
    let message = match run_fixer() {
        Ok(message) => message,
        Err(err) => format!("Failed to run Image Metadata Fixer:\n{err}"),
    };

    show_message(
        "Image Metadata Fixer",
        &message,
        message.starts_with("Failed"),
    );
}

fn run_fixer() -> Result<String, String> {
    let target = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| "No file or folder was provided.".to_string())?,
    );
    let target_is_file = target.is_file();

    let fixer = fixer_exe_path()?;
    let mut command = Command::new(&fixer);
    command
        .arg("fix")
        .arg("--max-depth")
        .arg("0")
        .arg("--summary-only")
        .arg(&target);

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

    Ok(clean_popup_message(&message, target_is_file))
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
