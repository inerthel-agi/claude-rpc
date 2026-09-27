// Opens Claude Desktop and Claude Code from the tray menu, or their download
// and install pages when they are missing, plus the fixed links of the
// settings About page. Only the URLs listed here can ever be opened.

use std::path::{Path, PathBuf};

pub const DESKTOP_DOWNLOAD_URL: &str = "https://claude.com/download";
pub const CODE_SETUP_URL: &str = "https://code.claude.com/docs/en/setup";

// Named links the settings window may open; anything else is refused.
pub fn link_url(name: &str) -> Option<&'static str> {
    match name {
        "repo" => Some("https://github.com/inerthel-agi/claude-rpc"),
        "profile" => Some("https://github.com/inerthel-agi"),
        "releases" => Some("https://github.com/inerthel-agi/claude-rpc/releases"),
        "issues" => Some("https://github.com/inerthel-agi/claude-rpc/issues"),
        "desktop" => Some(DESKTOP_DOWNLOAD_URL),
        "code" => Some(CODE_SETUP_URL),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub enum Desktop {
    // Microsoft Store (MSIX) package, started through its AppUserModelID.
    Store(String),
    // Classic per-user install (%LOCALAPPDATA%\AnthropicClaude\claude.exe).
    Exe(PathBuf),
}

pub fn find_desktop() -> Option<Desktop> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    find_desktop_in(&local)
}

fn find_desktop_in(local: &Path) -> Option<Desktop> {
    let classic = local.join("AnthropicClaude").join("claude.exe");
    if classic.is_file() {
        return Some(Desktop::Exe(classic));
    }
    // An MSIX install keeps a per-user data folder named after its package
    // family ("Claude_<publisher id>"); Windows removes it on uninstall.
    let family = std::fs::read_dir(local.join("Packages"))
        .ok()?
        .filter_map(Result::ok)
        // Cheap name test first: Packages can hold hundreds of folders.
        .filter(|entry| entry.file_name().to_str().is_some_and(is_claude_family))
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .next()?;
    Some(Desktop::Store(format!("{family}!Claude")))
}

fn is_claude_family(name: &str) -> bool {
    name.strip_prefix("Claude_").is_some_and(|publisher| {
        !publisher.is_empty() && publisher.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

// The `claude` command of Claude Code: on PATH, or in the native installer's
// default folder (~/.local/bin) when PATH was not refreshed yet.
pub fn find_code() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    find_code_in(std::env::split_paths(&path), home.as_deref())
}

fn find_code_in(dirs: impl Iterator<Item = PathBuf>, home: Option<&Path>) -> Option<PathBuf> {
    let native = home.map(|home| home.join(".local").join("bin"));
    dirs.chain(native)
        // App execution aliases live there; never mistake another app for Claude Code.
        .filter(|dir| !is_windows_apps(dir))
        .flat_map(|dir| ["claude.exe", "claude.cmd"].map(|name| dir.join(name)))
        .find(|path| path.is_file())
}

fn is_windows_apps(dir: &Path) -> bool {
    dir.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
        .ends_with("\\microsoft\\windowsapps")
}

// Session variables set by Claude Code / Claude Desktop for their children
// (CLAUDECODE, CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_SESSION_ID, CLAUDE_PID,
// ANTHROPIC_BASE_URL…). `name` is upper-case.
#[cfg(any(windows, test))]
fn is_session_var(name: &str) -> bool {
    name == "CLAUDECODE"
        || name == "CLAUDE_PID"
        || [
            "CLAUDE_CODE_",
            "CLAUDE_AGENT_SDK_",
            "CLAUDE_PREVIEW_",
            "ANTHROPIC_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

// Value names from `reg query` output ("    NAME    REG_SZ    value"), upper-case.
#[cfg(any(windows, test))]
fn reg_value_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| line.starts_with("    "))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            parts
                .next()
                .filter(|kind| kind.starts_with("REG_"))
                .map(|_| name.to_ascii_uppercase())
        })
        .collect()
}

#[cfg(windows)]
mod platform {
    use super::Desktop;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    pub fn open_url(url: &str) -> Result<(), String> {
        Command::new("explorer.exe")
            .arg(url)
            .spawn()
            .map(drop)
            .map_err(|err| err.to_string())
    }

    pub fn launch_desktop(desktop: &Desktop) -> Result<(), String> {
        match desktop {
            Desktop::Store(app_id) => Command::new("explorer.exe")
                .arg(format!("shell:AppsFolder\\{app_id}"))
                .spawn(),
            Desktop::Exe(path) => without_session_env(Command::new(path)).spawn(),
        }
        .map(drop)
        .map_err(|err| err.to_string())
    }

    // `claude` in the home folder, in Windows Terminal when it is installed
    // (full colors and symbols), else in a new console window. The shell stays
    // open (cmd /k) when Claude Code exits.
    pub fn launch_code(claude: &Path) -> Result<(), String> {
        let home = std::env::var_os("USERPROFILE");
        let terminal = std::env::var_os("LOCALAPPDATA")
            .map(|local| {
                std::path::PathBuf::from(local)
                    .join("Microsoft")
                    .join("WindowsApps")
                    .join("wt.exe")
            })
            .filter(|path| path.is_file())
            // wt.exe splits its command line on ';'.
            .filter(|_| !claude.to_string_lossy().contains(';'))
            .filter(|_| {
                home.as_ref()
                    .is_none_or(|home| !home.to_string_lossy().contains(';'))
            });
        let mut command = match terminal {
            Some(wt) => {
                let mut command = Command::new(wt);
                command.arg("new-tab");
                if let Some(home) = &home {
                    command.arg("-d").arg(home);
                }
                command.args(["cmd.exe", "/d", "/k", "call"]).arg(claude);
                command
            }
            None => {
                let mut command = Command::new("cmd.exe");
                command
                    // call keeps cmd from stripping the quotes of a path
                    // with spaces and ( ) & characters.
                    .raw_arg(format!("/d /k call \"{}\"", claude.display()))
                    .creation_flags(CREATE_NEW_CONSOLE);
                command
            }
        };
        if let Some(home) = home {
            command.current_dir(home);
        }
        without_session_env(command)
            .spawn()
            .map(drop)
            .map_err(|err| err.to_string())
    }

    // Drops the variables a Claude Code / Claude Desktop session sets for its
    // children (see super::is_session_var). They only exist when Claude RPC was
    // itself started from such a session; passed on, the new Claude Code would
    // run as a child session (no transcript, borrowed endpoints and tokens).
    // Variables the user set in Windows (registry environment) are kept.
    fn without_session_env(mut command: Command) -> Command {
        let persistent = persistent_env_names();
        for (name, _) in std::env::vars_os() {
            let upper = name.to_string_lossy().to_ascii_uppercase();
            if super::is_session_var(&upper) && !persistent.contains(&upper) {
                command.env_remove(&name);
            }
        }
        command
    }

    fn persistent_env_names() -> Vec<String> {
        const KEYS: [&str; 2] = [
            r"HKCU\Environment",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ];
        KEYS.iter()
            .filter_map(|key| {
                Command::new("reg.exe")
                    .args(["query", key])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .ok()
            })
            .flat_map(|output| super::reg_value_names(&String::from_utf8_lossy(&output.stdout)))
            .collect()
    }

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
}

#[cfg(not(windows))]
mod platform {
    use super::Desktop;
    use std::path::Path;

    pub fn open_url(_url: &str) -> Result<(), String> {
        Err("only supported on Windows".into())
    }

    pub fn launch_desktop(_desktop: &Desktop) -> Result<(), String> {
        Err("only supported on Windows".into())
    }

    pub fn launch_code(_claude: &Path) -> Result<(), String> {
        Err("only supported on Windows".into())
    }
}

pub use platform::open_url;

// Opens Claude Desktop, or its download page when it is not installed.
pub fn open_desktop() -> Result<(), String> {
    match find_desktop() {
        Some(desktop) => platform::launch_desktop(&desktop),
        None => open_url(DESKTOP_DOWNLOAD_URL),
    }
}

// Starts Claude Code in a terminal, or opens its install guide.
pub fn open_code() -> Result<(), String> {
    match find_code() {
        Some(claude) => platform::launch_code(&claude),
        None => open_url(CODE_SETUP_URL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("claude-rpc-launch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_known_links_open() {
        assert_eq!(
            link_url("repo"),
            Some("https://github.com/inerthel-agi/claude-rpc")
        );
        assert_eq!(link_url("profile"), Some("https://github.com/inerthel-agi"));
        assert_eq!(link_url("code"), Some(CODE_SETUP_URL));
        assert_eq!(link_url("https://example.com"), None);
        assert_eq!(link_url(""), None);
    }

    #[test]
    fn strips_only_session_variables() {
        for name in [
            "CLAUDECODE",
            "CLAUDE_PID",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_AGENT_SDK_VERSION",
            "ANTHROPIC_BASE_URL",
        ] {
            assert!(is_session_var(name), "{name}");
        }
        for name in ["PATH", "USERPROFILE", "CLAUDE_RPC_DIR", "CLAUDE_DIR_PATH"] {
            assert!(!is_session_var(name), "{name}");
        }

        let output = "\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_EXPAND_SZ    %USERPROFILE%\\bin\r\n    ANTHROPIC_API_KEY    REG_SZ    x\r\n    TEMP    REG_EXPAND_SZ    C:\\Temp\r\n\r\n";
        assert_eq!(
            reg_value_names(output),
            vec!["PATH", "ANTHROPIC_API_KEY", "TEMP"]
        );
    }

    #[test]
    fn finds_claude_desktop_installs() {
        let local = temp_dir("desktop");
        assert_eq!(find_desktop_in(&local), None);

        fs::create_dir_all(local.join("Packages").join("ClaudeHelper_x")).unwrap();
        fs::create_dir_all(local.join("Packages").join("Microsoft.Claude_abc")).unwrap();
        assert_eq!(find_desktop_in(&local), None);

        fs::create_dir_all(local.join("Packages").join("Claude_pzs8sxrjxfjjc")).unwrap();
        assert_eq!(
            find_desktop_in(&local),
            Some(Desktop::Store("Claude_pzs8sxrjxfjjc!Claude".into()))
        );

        let classic = local.join("AnthropicClaude");
        fs::create_dir_all(&classic).unwrap();
        fs::write(classic.join("claude.exe"), b"").unwrap();
        assert_eq!(
            find_desktop_in(&local),
            Some(Desktop::Exe(classic.join("claude.exe")))
        );
        let _ = fs::remove_dir_all(&local);
    }

    #[test]
    fn finds_claude_code_on_path() {
        let root = temp_dir("code");
        let npm = root.join("npm");
        let aliases = root.join("Microsoft").join("WindowsApps");
        let home = root.join("home");
        for dir in [&npm, &aliases, &home] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(aliases.join("claude.exe"), b"").unwrap();
        assert_eq!(
            find_code_in([aliases.clone(), npm.clone()].into_iter(), Some(&home)),
            None
        );

        let native = home.join(".local").join("bin");
        fs::create_dir_all(&native).unwrap();
        fs::write(native.join("claude.exe"), b"").unwrap();
        assert_eq!(
            find_code_in([npm.clone()].into_iter(), Some(&home)),
            Some(native.join("claude.exe"))
        );

        fs::write(npm.join("claude.cmd"), b"").unwrap();
        assert_eq!(
            find_code_in([npm.clone()].into_iter(), Some(&home)),
            Some(npm.join("claude.cmd"))
        );
        let _ = fs::remove_dir_all(&root);
    }
}
