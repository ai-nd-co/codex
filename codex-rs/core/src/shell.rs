use crate::shell_detect::detect_shell_type;
use crate::shell_snapshot::ShellSnapshot;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum ShellType {
    Zsh,
    Bash,
    PowerShell,
    Sh,
    Cmd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shell {
    pub(crate) shell_type: ShellType,
    pub(crate) shell_path: PathBuf,
    #[serde(
        skip_serializing,
        skip_deserializing,
        default = "empty_shell_snapshot_receiver"
    )]
    pub(crate) shell_snapshot: watch::Receiver<Option<Arc<ShellSnapshot>>>,
}

impl Shell {
    pub fn name(&self) -> &'static str {
        match self.shell_type {
            ShellType::Zsh => "zsh",
            ShellType::Bash => "bash",
            ShellType::PowerShell => "powershell",
            ShellType::Sh => "sh",
            ShellType::Cmd => "cmd",
        }
    }

    /// Takes a string of shell and returns the full list of command args to
    /// use with `exec()` to run the shell command.
    pub fn derive_exec_args(&self, command: &str, use_login_shell: bool) -> Vec<String> {
        match self.shell_type {
            ShellType::Zsh | ShellType::Bash | ShellType::Sh => {
                let arg = if use_login_shell { "-lc" } else { "-c" };
                vec![
                    self.shell_path.to_string_lossy().to_string(),
                    arg.to_string(),
                    command.to_string(),
                ]
            }
            ShellType::PowerShell => {
                let mut args = vec![self.shell_path.to_string_lossy().to_string()];
                if !use_login_shell {
                    args.push("-NoProfile".to_string());
                }

                args.push("-Command".to_string());
                args.push(command.to_string());
                args
            }
            ShellType::Cmd => {
                let mut args = vec![self.shell_path.to_string_lossy().to_string()];
                args.push("/c".to_string());
                args.push(command.to_string());
                args
            }
        }
    }

    /// Return the shell snapshot if existing.
    pub fn shell_snapshot(&self) -> Option<Arc<ShellSnapshot>> {
        self.shell_snapshot.borrow().clone()
    }
}

pub(crate) fn empty_shell_snapshot_receiver() -> watch::Receiver<Option<Arc<ShellSnapshot>>> {
    let (_tx, rx) = watch::channel(None);
    rx
}

impl PartialEq for Shell {
    fn eq(&self, other: &Self) -> bool {
        self.shell_type == other.shell_type && self.shell_path == other.shell_path
    }
}

impl Eq for Shell {}

#[cfg(unix)]
fn get_user_shell_path() -> Option<PathBuf> {
    let uid = unsafe { libc::getuid() };
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::ptr;

    let mut passwd = MaybeUninit::<libc::passwd>::uninit();

    // We cannot use getpwuid here: it returns pointers into libc-managed
    // storage, which is not safe to read concurrently on all targets (the musl
    // static build used by the CLI can segfault when parallel callers race on
    // that buffer). getpwuid_r keeps the passwd data in caller-owned memory.
    let suggested_buffer_len = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let buffer_len = usize::try_from(suggested_buffer_len)
        .ok()
        .filter(|len| *len > 0)
        .unwrap_or(1024);
    let mut buffer = vec![0; buffer_len];

    loop {
        let mut result = ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                passwd.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            if result.is_null() {
                return None;
            }

            let passwd = unsafe { passwd.assume_init_ref() };
            if passwd.pw_shell.is_null() {
                return None;
            }

            let shell_path = unsafe { CStr::from_ptr(passwd.pw_shell) }
                .to_string_lossy()
                .into_owned();
            return Some(PathBuf::from(shell_path));
        }

        if status != libc::ERANGE {
            return None;
        }

        // Retry with a larger buffer until libc can materialize the passwd entry.
        let new_len = buffer.len().checked_mul(2)?;
        if new_len > 1024 * 1024 {
            return None;
        }
        buffer.resize(new_len, 0);
    }
}

#[cfg(not(unix))]
fn get_user_shell_path() -> Option<PathBuf> {
    None
}

fn file_exists(path: &PathBuf) -> Option<PathBuf> {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        Some(PathBuf::from(path))
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn is_wsl_bash_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().to_lowercase().replace('/', "\\");
    normalized.ends_with("\\windows\\system32\\bash.exe")
        || normalized.ends_with("\\windows\\sysnative\\bash.exe")
        || normalized.ends_with("\\windows\\syswow64\\bash.exe")
}

fn get_shell_path(
    shell_type: ShellType,
    provided_path: Option<&PathBuf>,
    binary_name: &str,
    fallback_paths: &[&str],
) -> Option<PathBuf> {
    // If exact provided path exists, use it
    if provided_path.and_then(file_exists).is_some() {
        return provided_path.cloned();
    }

    // Check if the shell we are trying to load is user's default shell
    // if just use it
    let default_shell_path = get_user_shell_path();
    if let Some(default_shell_path) = default_shell_path
        && detect_shell_type(&default_shell_path) == Some(shell_type)
        && file_exists(&default_shell_path).is_some()
    {
        return Some(default_shell_path);
    }

    if let Ok(path) = which::which(binary_name) {
        return Some(path);
    }

    for path in fallback_paths {
        //check exists
        if let Some(path) = file_exists(&PathBuf::from(path)) {
            return Some(path);
        }
    }

    None
}

const ZSH_FALLBACK_PATHS: &[&str] = &["/bin/zsh"];

fn get_zsh_shell(path: Option<&PathBuf>) -> Option<Shell> {
    let shell_path = get_shell_path(ShellType::Zsh, path, "zsh", ZSH_FALLBACK_PATHS);

    shell_path.map(|shell_path| Shell {
        shell_type: ShellType::Zsh,
        shell_path,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

#[cfg(not(target_os = "windows"))]
const BASH_FALLBACK_PATHS: &[&str] = &["/bin/bash"];

#[cfg(target_os = "windows")]
fn get_bash_shell(path: Option<&PathBuf>) -> Option<Shell> {
    fn make_shell(shell_path: PathBuf) -> Shell {
        Shell {
            shell_type: ShellType::Bash,
            shell_path,
            shell_snapshot: empty_shell_snapshot_receiver(),
        }
    }

    if let Some(path) = path.and_then(file_exists) {
        return Some(make_shell(path));
    }

    for candidate in [
        r#"C:\Program Files\Git\bin\bash.exe"#,
        r#"C:\Program Files\Git\usr\bin\bash.exe"#,
        r#"C:\Program Files (x86)\Git\bin\bash.exe"#,
        r#"C:\Program Files (x86)\Git\usr\bin\bash.exe"#,
    ] {
        if let Some(path) = file_exists(&PathBuf::from(candidate)) {
            return Some(make_shell(path));
        }
    }

    if let Ok(path) = which::which("bash")
        && !is_wsl_bash_path(&path)
    {
        return Some(make_shell(path));
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn get_bash_shell(path: Option<&PathBuf>) -> Option<Shell> {
    let shell_path = get_shell_path(ShellType::Bash, path, "bash", BASH_FALLBACK_PATHS);

    shell_path.map(|shell_path| Shell {
        shell_type: ShellType::Bash,
        shell_path,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

const SH_FALLBACK_PATHS: &[&str] = &["/bin/sh"];

fn get_sh_shell(path: Option<&PathBuf>) -> Option<Shell> {
    let shell_path = get_shell_path(ShellType::Sh, path, "sh", SH_FALLBACK_PATHS);

    shell_path.map(|shell_path| Shell {
        shell_type: ShellType::Sh,
        shell_path,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

// Note the `pwsh` and `powershell` fallback paths are where the respective
// shells are commonly installed on GitHub Actions Windows runners, but may not
// be present on all Windows machines:
// https://docs.github.com/en/actions/tutorials/build-and-test-code/powershell

#[cfg(windows)]
const PWSH_FALLBACK_PATHS: &[&str] = &[r#"C:\Program Files\PowerShell\7\pwsh.exe"#];
#[cfg(not(windows))]
const PWSH_FALLBACK_PATHS: &[&str] = &["/usr/local/bin/pwsh"];

#[cfg(windows)]
const POWERSHELL_FALLBACK_PATHS: &[&str] =
    &[r#"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"#];
#[cfg(not(windows))]
const POWERSHELL_FALLBACK_PATHS: &[&str] = &[];

fn get_powershell_shell(path: Option<&PathBuf>) -> Option<Shell> {
    let shell_path = get_shell_path(ShellType::PowerShell, path, "pwsh", PWSH_FALLBACK_PATHS)
        .or_else(|| {
            get_shell_path(
                ShellType::PowerShell,
                path,
                "powershell",
                POWERSHELL_FALLBACK_PATHS,
            )
        });

    shell_path.map(|shell_path| Shell {
        shell_type: ShellType::PowerShell,
        shell_path,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

fn get_cmd_shell(path: Option<&PathBuf>) -> Option<Shell> {
    let shell_path = get_shell_path(ShellType::Cmd, path, "cmd", &[]);

    shell_path.map(|shell_path| Shell {
        shell_type: ShellType::Cmd,
        shell_path,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

fn ultimate_fallback_shell() -> Shell {
    if cfg!(windows) {
        Shell {
            shell_type: ShellType::Cmd,
            shell_path: PathBuf::from("cmd.exe"),
            shell_snapshot: empty_shell_snapshot_receiver(),
        }
    } else {
        Shell {
            shell_type: ShellType::Sh,
            shell_path: PathBuf::from("/bin/sh"),
            shell_snapshot: empty_shell_snapshot_receiver(),
        }
    }
}

pub fn get_shell_by_model_provided_path(shell_path: &PathBuf) -> Shell {
    detect_shell_type(shell_path)
        .and_then(|shell_type| get_shell(shell_type, Some(shell_path)))
        .unwrap_or(ultimate_fallback_shell())
}

pub fn get_shell(shell_type: ShellType, path: Option<&PathBuf>) -> Option<Shell> {
    match shell_type {
        ShellType::Zsh => get_zsh_shell(path),
        ShellType::Bash => get_bash_shell(path),
        ShellType::PowerShell => get_powershell_shell(path),
        ShellType::Sh => get_sh_shell(path),
        ShellType::Cmd => get_cmd_shell(path),
    }
}

pub fn default_user_shell() -> Shell {
    let shell_path_override = shell_path_override_from_env();
    default_user_shell_with_override(shell_path_override.as_ref())
}

fn default_user_shell_from_path(user_shell_path: Option<PathBuf>) -> Shell {
    if cfg!(windows) {
        get_shell(ShellType::Bash, /*path*/ None)
            .or_else(|| get_shell(ShellType::PowerShell, /*path*/ None))
            .unwrap_or(ultimate_fallback_shell())
    } else {
        let user_default_shell = user_shell_path
            .and_then(|shell| detect_shell_type(&shell))
            .and_then(|shell_type| get_shell(shell_type, /*path*/ None));

        let shell_with_fallback = if cfg!(target_os = "macos") {
            user_default_shell
                .or_else(|| get_shell(ShellType::Zsh, /*path*/ None))
                .or_else(|| get_shell(ShellType::Bash, /*path*/ None))
        } else {
            user_default_shell
                .or_else(|| get_shell(ShellType::Bash, /*path*/ None))
                .or_else(|| get_shell(ShellType::Zsh, /*path*/ None))
        };

        shell_with_fallback.unwrap_or(ultimate_fallback_shell())
    }
}

pub fn default_user_shell_with_override(shell_path_override: Option<&PathBuf>) -> Shell {
    shell_path_override
        .map(get_shell_by_model_provided_path)
        .unwrap_or_else(default_user_shell_from_env)
}

fn default_user_shell_from_env() -> Shell {
    default_user_shell_from_path(get_user_shell_path())
}

fn shell_path_override_from_env() -> Option<PathBuf> {
    std::env::var_os("CODEX_SHELL_PATH")
        .map(PathBuf::from)
        .and_then(|path| file_exists(&path))
        .or_else(|| {
            std::env::var_os("CODEX_GIT_BASH_PATH")
                .map(PathBuf::from)
                .and_then(|path| file_exists(&path))
        })
}

#[cfg(test)]
mod detect_shell_type_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_shell_type() {
        assert_eq!(
            detect_shell_type(&PathBuf::from("zsh")),
            Some(ShellType::Zsh)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("bash")),
            Some(ShellType::Bash)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("pwsh")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("powershell")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(detect_shell_type(&PathBuf::from("fish")), None);
        assert_eq!(detect_shell_type(&PathBuf::from("other")), None);
        assert_eq!(
            detect_shell_type(&PathBuf::from("/bin/zsh")),
            Some(ShellType::Zsh)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("/bin/bash")),
            Some(ShellType::Bash)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("powershell.exe")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from(if cfg!(windows) {
                "C:\\windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
            } else {
                "/usr/local/bin/pwsh"
            })),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("pwsh.exe")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("/usr/local/bin/pwsh")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("/bin/sh")),
            Some(ShellType::Sh)
        );
        assert_eq!(detect_shell_type(&PathBuf::from("sh")), Some(ShellType::Sh));
        assert_eq!(
            detect_shell_type(&PathBuf::from("cmd")),
            Some(ShellType::Cmd)
        );
        assert_eq!(
            detect_shell_type(&PathBuf::from("cmd.exe")),
            Some(ShellType::Cmd)
        );
    }

    #[cfg(windows)]
    #[test]
    fn detects_wsl_bash_paths() {
        assert!(is_wsl_bash_path(&PathBuf::from(
            r"C:\Windows\System32\bash.exe"
        )));
        assert!(is_wsl_bash_path(&PathBuf::from(
            r"C:\Windows\Sysnative\bash.exe"
        )));
        assert!(!is_wsl_bash_path(&PathBuf::from(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
    }

    #[test]
    fn honors_shell_override_path_for_bash() {
        let tmp = tempdir().unwrap();
        let shell_path = tmp.path().join("bash.exe");
        std::fs::write(&shell_path, "").unwrap();

        let shell = default_user_shell_with_override(Some(&shell_path));

        assert_eq!(shell.shell_type, ShellType::Bash);
        assert_eq!(shell.shell_path, shell_path);
    }

    #[cfg(windows)]
    #[test]
    fn detects_windows_default_shell() {
        let shell = default_user_shell();
        let shell_path = shell.shell_path;

        match shell.shell_type {
            ShellType::Bash => {
                assert!(
                    shell_path.ends_with("bash.exe"),
                    "shell path: {shell_path:?}"
                );
                assert!(
                    !is_wsl_bash_path(&shell_path),
                    "default bash shell should not be the WSL shim: {shell_path:?}"
                );
            }
            ShellType::PowerShell => {
                assert!(
                    shell_path.ends_with("pwsh.exe") || shell_path.ends_with("powershell.exe"),
                    "shell path: {shell_path:?}"
                );
            }
            other => panic!("unexpected windows default shell: {other:?}"),
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
#[path = "shell_tests.rs"]
mod tests;
