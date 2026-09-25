use super::*;

#[cfg(windows)]
pub(super) struct ProcessEntry {
    process_id: u32,
    name: String,
}

#[cfg(target_os = "macos")]
pub(super) struct MacProcessEntry {
    process_id: u32,
    name: String,
    command: String,
}

#[derive(Debug, Clone)]
pub(super) struct ProcessSnapshot {
    pub(super) process_id: u32,
    pub(super) name: String,
    pub(super) executable_path: Option<String>,
    pub(super) command_line: Option<String>,
    pub(super) creation_date_ms: Option<u64>,
}

#[cfg(windows)]
pub(super) fn scan_claude_processes() -> Vec<ProcessSnapshot> {
    list_process_entries()
        .into_iter()
        .filter(|entry| {
            entry.name.eq_ignore_ascii_case("claude.exe")
                || entry.name.eq_ignore_ascii_case("claude desktop.exe")
        })
        .map(|entry| ProcessSnapshot {
            process_id: entry.process_id,
            name: entry.name,
            executable_path: query_process_path(entry.process_id),
            command_line: query_process_command_line(entry.process_id),
            creation_date_ms: query_process_creation_ms(entry.process_id),
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn scan_claude_processes() -> Vec<ProcessSnapshot> {
    list_macos_process_entries()
        .into_iter()
        .filter(|entry| is_macos_claude_candidate(&entry.name, &entry.command))
        .map(|entry| ProcessSnapshot {
            process_id: entry.process_id,
            name: entry.name,
            executable_path: Some(entry.command.clone()),
            command_line: Some(entry.command),
            creation_date_ms: None,
        })
        .collect()
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub(super) fn scan_claude_processes() -> Vec<ProcessSnapshot> {
    Vec::new()
}

pub(super) fn is_desktop_process(process: &ProcessSnapshot) -> bool {
    if process
        .command_line
        .as_deref()
        .is_some_and(is_background_helper)
    {
        return false;
    }
    let exe = process
        .executable_path
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let exe_unix = exe.replace('\\', "/");
    process.name.eq_ignore_ascii_case("claude desktop.exe")
        || process.name.eq_ignore_ascii_case("claude desktop")
        || (process.name.eq_ignore_ascii_case("claude")
            && exe_unix.contains(".app/contents/macos/"))
        || exe.contains("windowsapps")
        || exe.contains("anthropicclaude")
        || exe.contains("\\program files\\claude")
        || exe.contains("\\appdata\\local\\anthropic")
        || exe_unix.contains("/applications/claude.app/")
        || exe_unix.contains("/claude.app/contents/macos/")
        || exe_unix.contains("/library/application support/claude/")
}

pub(super) fn is_code_process(process: &ProcessSnapshot) -> bool {
    if process
        .command_line
        .as_deref()
        .is_some_and(is_background_helper)
    {
        return false;
    }
    let exe = process
        .executable_path
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace('\\', "/");
    (process.name.eq_ignore_ascii_case("claude.exe")
        || process.name.eq_ignore_ascii_case("claude")
        || exe.contains("/node_modules/@anthropic-ai/claude-code/")
        || exe.contains("/node_modules/claude-code/")
        || exe.contains("/.claude/local/")
        || exe.contains("/claude-code/"))
        && !is_desktop_process(process)
}

// Chrome keeps this helper running even when no Claude Code session is open.
// Inspect the launch argument, not text passed later as a user prompt.
fn is_chrome_native_host(command: &str) -> bool {
    let command = command.trim_start();
    let args = if let Some(quoted) = command.strip_prefix('"') {
        quoted.split_once('"').map(|(_, args)| args)
    } else {
        command
            .split_once(char::is_whitespace)
            .map(|(_, args)| args)
    };
    args.and_then(|args| args.split_whitespace().next())
        .is_some_and(|arg| arg.trim_matches('"') == "--chrome-native-host")
}

fn is_background_helper(command: &str) -> bool {
    if is_chrome_native_host(command) {
        return true;
    }

    // claude-mem keeps a stateless SDK worker alive without a user session.
    // Keep quoted prompt text intact so mentioning these flags cannot hide a CLI.
    let mut quote = None;
    let mut escaped = false;
    let mut args = command
        .split(|ch: char| {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() && matches!(ch, '\'' | '"') {
                quote = Some(ch);
            }
            quote.is_none() && ch.is_whitespace()
        })
        .filter(|arg| !arg.is_empty())
        .skip(1)
        .map(|arg| arg.trim_matches(['\'', '"']))
        .take_while(|arg| *arg != "--");
    let mut stateless = false;
    let mut streaming_input = false;
    while let Some(arg) = args.next() {
        match arg {
            "--no-session-persistence" => stateless = true,
            "--input-format" => streaming_input = args.next() == Some("stream-json"),
            "--input-format=stream-json" => streaming_input = true,
            _ => {}
        }
    }
    stateless && streaming_input
}

#[cfg(windows)]
fn query_process_command_line(process_id: u32) -> Option<String> {
    use windows::{
        Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation},
        Win32::Foundation::UNICODE_STRING,
    };

    let handle = open_process_query(process_id)?;
    // A Windows command line is bounded by 32,767 UTF-16 characters.
    let mut buffer = vec![0u16; 32_768 + std::mem::size_of::<UNICODE_STRING>() / 2];
    let mut returned = 0;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 2) as u32,
            &mut returned,
        )
    };
    close_handle(handle);
    status.ok().ok()?;
    let value = unsafe { buffer.as_ptr().cast::<UNICODE_STRING>().read_unaligned() };
    let offset = (value.Buffer.0 as usize).checked_sub(buffer.as_ptr() as usize)?;
    let end = offset.checked_add(value.Length as usize)?;
    if offset < std::mem::size_of::<UNICODE_STRING>()
        || offset % 2 != 0
        || value.Length % 2 != 0
        || end > returned as usize
        || end > buffer.len() * 2
    {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[offset / 2..end / 2]))
}

#[cfg(windows)]
pub(super) fn list_process_entries() -> Vec<ProcessEntry> {
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            entries.push(ProcessEntry {
                process_id: entry.th32ProcessID,
                name: wide_to_string(&entry.szExeFile),
            });

            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }

    close_handle(snapshot);
    entries
}

#[cfg(windows)]
pub(super) fn query_process_path(process_id: u32) -> Option<String> {
    let handle = open_process_query(process_id)?;
    let mut buffer = vec![0u16; 32_768];
    let mut len = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    close_handle(handle);
    result.ok()?;
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
}

#[cfg(windows)]
pub(super) fn query_process_creation_ms(process_id: u32) -> Option<u64> {
    let handle = open_process_query(process_id)?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    close_handle(handle);
    result.ok()?;
    filetime_to_unix_ms(creation)
}

#[cfg(windows)]
pub(super) fn open_process_query(process_id: u32) -> Option<HANDLE> {
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()
}

#[cfg(windows)]
pub(super) fn close_handle(handle: HANDLE) {
    let _ = unsafe { CloseHandle(handle) };
}

#[cfg(windows)]
pub(super) fn wide_to_string(value: &[u16]) -> String {
    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

#[cfg(windows)]
pub(super) fn filetime_to_unix_ms(value: FILETIME) -> Option<u64> {
    const WINDOWS_TO_UNIX_EPOCH_MS: u64 = 11_644_473_600_000;
    let ticks = ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64;
    let ms = ticks / 10_000;
    ms.checked_sub(WINDOWS_TO_UNIX_EPOCH_MS)
}

#[cfg(target_os = "macos")]
pub(super) fn list_macos_process_entries() -> Vec<MacProcessEntry> {
    let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,comm=,command="])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_macos_process_line)
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn parse_macos_process_line(line: &str) -> Option<MacProcessEntry> {
    let (process_id, rest) = split_process_field(line)?;
    let (comm, rest) = split_process_field(rest)?;
    let process_id = process_id.parse().ok()?;
    let command = rest.trim_start().to_string();
    if command.is_empty() {
        return None;
    }
    Some(MacProcessEntry {
        process_id,
        name: command_basename(comm),
        command,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn split_process_field(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    if input.is_empty() {
        return None;
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    Some((&input[..end], &input[end..]))
}

#[cfg(target_os = "macos")]
pub(super) fn is_macos_claude_candidate(name: &str, command: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let command = command.to_ascii_lowercase();
    if command.contains("claude-rpc") {
        return false;
    }
    name == "claude"
        || name == "claude desktop"
        || command.contains("/applications/claude.app/")
        || command.contains("/claude.app/contents/macos/")
        || command.contains("/node_modules/@anthropic-ai/claude-code/")
        || command.contains("/node_modules/claude-code/")
        || command.contains("/.claude/local/")
        || command.contains("/claude-code/")
}

#[cfg(target_os = "macos")]
pub(super) fn command_basename(command: &str) -> String {
    let executable = command.split_whitespace().next().unwrap_or(command);
    executable
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(executable)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_stateless_sdk_workers_but_keeps_user_clients() {
        let mut process = ProcessSnapshot {
            process_id: 1,
            name: "claude.exe".into(),
            executable_path: Some(r"C:\Users\test\.local\bin\claude.exe".into()),
            command_line: None,
            creation_date_ms: None,
        };
        for command in [
            "claude.exe --output-format stream-json --verbose --input-format stream-json --model sonnet --permission-prompt-tool stdio --permission-mode dontAsk --no-session-persistence",
            r#""C:\Users\Test User\.local\bin\claude.exe" --no-session-persistence --input-format "stream-json""#,
            "/usr/local/bin/claude --no-session-persistence --input-format=stream-json",
        ] {
            process.command_line = Some(command.into());
            assert!(!is_code_process(&process), "{command}");
            assert!(!is_desktop_process(&process), "{command}");
        }
        for command in [
            "claude.exe --resume",
            "claude.exe --input-format stream-json",
            "claude.exe -p --no-session-persistence explain",
            r#"claude.exe -p "Explain --input-format stream-json --no-session-persistence""#,
            r#"claude.exe -p "Explain \"flags\" --no-session-persistence" --input-format stream-json"#,
            "claude.exe -- --input-format stream-json --no-session-persistence",
        ] {
            process.command_line = Some(command.into());
            assert!(is_code_process(&process), "{command}");
        }
        process.executable_path = Some(r"C:\Program Files\Claude\Claude.exe".into());
        process.command_line = Some(r#""C:\Program Files\Claude\Claude.exe""#.into());
        assert!(is_desktop_process(&process));
        assert!(!is_code_process(&process));
    }

    #[test]
    fn excludes_chrome_host_but_keeps_code_sessions() {
        let mut process = ProcessSnapshot {
            process_id: 1,
            name: "claude.exe".into(),
            executable_path: Some(r"C:\Users\test\.local\bin\claude.exe".into()),
            command_line: None,
            creation_date_ms: None,
        };
        for command in [
            r"C:\Users\test\.local\bin\claude.exe --chrome-native-host",
            r#""C:\Users\Test User\.local\bin\claude.exe" --chrome-native-host"#,
            "/usr/local/bin/claude --chrome-native-host",
        ] {
            process.command_line = Some(command.into());
            assert!(!is_code_process(&process), "{command}");
            assert!(!is_desktop_process(&process), "{command}");
        }
        for command in [
            "claude.exe",
            "claude.exe --resume",
            r#"claude.exe -p "Explain --chrome-native-host""#,
            "claude.exe -- --chrome-native-host",
        ] {
            process.command_line = Some(command.into());
            assert!(is_code_process(&process), "{command}");
        }
        process.command_line = None;
        assert!(is_code_process(&process));
    }
}
