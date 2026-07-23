use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum ShellType {
    Zsh,
    Bash,
    PowerShell,
    Sh,
    Cmd,
}

impl ShellType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
            Self::Sh => "sh",
            Self::Cmd => "cmd",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedShell {
    pub shell_type: ShellType,
    pub shell_path: PathBuf,
}

impl DetectedShell {
    pub fn name(&self) -> &'static str {
        self.shell_type.name()
    }
}

pub fn detect_shell_type(shell_path: impl AsRef<std::path::Path>) -> Option<ShellType> {
    let shell_path = shell_path.as_ref();
    match shell_path.as_os_str().to_str() {
        Some("zsh") => Some(ShellType::Zsh),
        Some("sh") => Some(ShellType::Sh),
        Some("cmd") => Some(ShellType::Cmd),
        Some("bash") => Some(ShellType::Bash),
        Some("pwsh") => Some(ShellType::PowerShell),
        Some("powershell") => Some(ShellType::PowerShell),
        _ => {
            let shell_name = shell_path.file_stem();
            if let Some(shell_name) = shell_name {
                let shell_name_path = std::path::Path::new(shell_name);
                if shell_name_path != shell_path {
                    return detect_shell_type(shell_name_path);
                }
            }
            None
        }
    }
}

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

fn file_exists(path: &std::path::Path) -> Option<PathBuf> {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        Some(PathBuf::from(path))
    } else {
        None
    }
}

#[cfg(windows)]
fn is_wsl_bash_path(path: &std::path::Path) -> bool {
    let normalized = path.to_string_lossy().to_lowercase().replace('/', "\\");
    normalized.ends_with("\\windows\\system32\\bash.exe")
        || normalized.ends_with("\\windows\\sysnative\\bash.exe")
        || normalized.ends_with("\\windows\\syswow64\\bash.exe")
}

#[cfg(windows)]
const WINDOWS_GIT_BASH_FALLBACK_PATHS: &[&str] = &[
    r#"C:\Program Files\Git\bin\bash.exe"#,
    r#"C:\Program Files\Git\usr\bin\bash.exe"#,
    r#"C:\Program Files (x86)\Git\bin\bash.exe"#,
    r#"C:\Program Files (x86)\Git\usr\bin\bash.exe"#,
];

fn get_shell_path(
    shell_type: ShellType,
    provided_path: Option<&PathBuf>,
    binary_name: &str,
    fallback_paths: &[&str],
) -> Option<PathBuf> {
    if let Some(path) = provided_path.and_then(|path| file_exists(path)) {
        return Some(path);
    }

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
        if let Some(path) = file_exists(std::path::Path::new(path)) {
            return Some(path);
        }
    }

    None
}

const ZSH_FALLBACK_PATHS: &[&str] = &["/bin/zsh"];

fn get_zsh_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    let shell_path = get_shell_path(ShellType::Zsh, path, "zsh", ZSH_FALLBACK_PATHS);

    shell_path.map(|shell_path| DetectedShell {
        shell_type: ShellType::Zsh,
        shell_path,
    })
}

#[cfg(not(windows))]
const BASH_FALLBACK_PATHS: &[&str] = &["/bin/bash", "/usr/bin/bash"];

#[cfg(windows)]
fn resolve_windows_bash_path<FExists, FWhichAll, I>(
    provided_path: Option<&PathBuf>,
    file_exists_fn: &FExists,
    which_bash_paths_fn: &FWhichAll,
) -> Option<PathBuf>
where
    FExists: Fn(&std::path::Path) -> Option<PathBuf>,
    FWhichAll: Fn() -> I,
    I: IntoIterator<Item = PathBuf>,
{
    if let Some(path) = provided_path.and_then(|path| file_exists_fn(path.as_path())) {
        return Some(path);
    }

    for candidate in WINDOWS_GIT_BASH_FALLBACK_PATHS {
        if let Some(path) = file_exists_fn(std::path::Path::new(candidate)) {
            return Some(path);
        }
    }

    for path in which_bash_paths_fn() {
        if !is_wsl_bash_path(&path) {
            return Some(path);
        }
    }

    None
}

#[cfg(windows)]
fn get_bash_shell_with_resolvers<FExists, FWhichAll, I>(
    provided_path: Option<&PathBuf>,
    file_exists_fn: &FExists,
    which_bash_paths_fn: &FWhichAll,
) -> Option<DetectedShell>
where
    FExists: Fn(&std::path::Path) -> Option<PathBuf>,
    FWhichAll: Fn() -> I,
    I: IntoIterator<Item = PathBuf>,
{
    resolve_windows_bash_path(provided_path, file_exists_fn, which_bash_paths_fn).map(
        |shell_path| DetectedShell {
            shell_type: ShellType::Bash,
            shell_path,
        },
    )
}

#[cfg(windows)]
fn which_all_bash_paths() -> Vec<PathBuf> {
    which::which_all("bash")
        .map(|paths| paths.collect())
        .unwrap_or_default()
}

#[cfg(windows)]
fn get_bash_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    get_bash_shell_with_resolvers(path, &file_exists, &which_all_bash_paths)
}

#[cfg(not(windows))]
fn get_bash_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    let shell_path = get_shell_path(ShellType::Bash, path, "bash", BASH_FALLBACK_PATHS);

    shell_path.map(|shell_path| DetectedShell {
        shell_type: ShellType::Bash,
        shell_path,
    })
}

const SH_FALLBACK_PATHS: &[&str] = &["/bin/sh"];

fn get_sh_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    let shell_path = get_shell_path(ShellType::Sh, path, "sh", SH_FALLBACK_PATHS);

    shell_path.map(|shell_path| DetectedShell {
        shell_type: ShellType::Sh,
        shell_path,
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

fn get_powershell_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    let shell_path = get_shell_path(ShellType::PowerShell, path, "pwsh", PWSH_FALLBACK_PATHS)
        .or_else(|| {
            get_shell_path(
                ShellType::PowerShell,
                path,
                "powershell",
                POWERSHELL_FALLBACK_PATHS,
            )
        });

    shell_path.map(|shell_path| DetectedShell {
        shell_type: ShellType::PowerShell,
        shell_path,
    })
}

fn get_cmd_shell(path: Option<&PathBuf>) -> Option<DetectedShell> {
    let shell_path = get_shell_path(ShellType::Cmd, path, "cmd", &[]);

    shell_path.map(|shell_path| DetectedShell {
        shell_type: ShellType::Cmd,
        shell_path,
    })
}

pub fn ultimate_fallback_shell() -> DetectedShell {
    if cfg!(windows) {
        DetectedShell {
            shell_type: ShellType::Cmd,
            shell_path: PathBuf::from("cmd.exe"),
        }
    } else {
        DetectedShell {
            shell_type: ShellType::Sh,
            shell_path: PathBuf::from("/bin/sh"),
        }
    }
}

pub fn get_shell_by_model_provided_path(shell_path: &PathBuf) -> DetectedShell {
    detect_shell_type(shell_path)
        .and_then(|shell_type| get_shell(shell_type, Some(shell_path)))
        .unwrap_or_else(ultimate_fallback_shell)
}

pub fn get_shell(shell_type: ShellType, path: Option<&PathBuf>) -> Option<DetectedShell> {
    match shell_type {
        ShellType::Zsh => get_zsh_shell(path),
        ShellType::Bash => get_bash_shell(path),
        ShellType::PowerShell => get_powershell_shell(path),
        ShellType::Sh => get_sh_shell(path),
        ShellType::Cmd => get_cmd_shell(path),
    }
}

pub fn default_user_shell() -> DetectedShell {
    let shell_path_override = shell_path_override_from_env();
    default_user_shell_with_override(shell_path_override.as_ref())
}

pub fn default_user_shell_from_path(user_shell_path: Option<PathBuf>) -> DetectedShell {
    if cfg!(windows) {
        default_windows_shell()
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

        shell_with_fallback.unwrap_or_else(ultimate_fallback_shell)
    }
}

fn default_user_shell_from_env() -> DetectedShell {
    default_user_shell_from_path(get_user_shell_path())
}

fn default_windows_shell() -> DetectedShell {
    default_windows_shell_with_resolvers(
        || get_shell(ShellType::Bash, /*path*/ None),
        || get_shell(ShellType::PowerShell, /*path*/ None),
    )
}

fn default_windows_shell_with_resolvers<FBash, FPowerShell>(
    get_bash_shell_fn: FBash,
    get_powershell_shell_fn: FPowerShell,
) -> DetectedShell
where
    FBash: FnOnce() -> Option<DetectedShell>,
    FPowerShell: FnOnce() -> Option<DetectedShell>,
{
    get_bash_shell_fn()
        .or_else(get_powershell_shell_fn)
        .unwrap_or_else(ultimate_fallback_shell)
}

fn default_user_shell_with_override(shell_path_override: Option<&PathBuf>) -> DetectedShell {
    shell_path_override
        .map(get_shell_by_model_provided_path)
        .unwrap_or_else(default_user_shell_from_env)
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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serial_test::serial;
    #[cfg(windows)]
    use std::ffi::{OsStr, OsString};
    use tempfile::tempdir;

    #[cfg(windows)]
    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    #[cfg(windows)]
    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let original = std::env::var_os(key);
            // SAFETY: test-only scoped env mutation guarded by serial_test.
            unsafe {
                std::env::set_var(key, value.as_ref());
            }
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            // SAFETY: test-only scoped env mutation guarded by serial_test.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, original }
        }
    }

    #[cfg(windows)]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: paired cleanup for the scoped env mutation above.
            unsafe {
                match &self.original {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn test_detect_shell_type() {
        assert_eq!(
            detect_shell_type(PathBuf::from("zsh")),
            Some(ShellType::Zsh)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("bash")),
            Some(ShellType::Bash)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("pwsh")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("powershell")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(detect_shell_type(PathBuf::from("fish")), None);
        assert_eq!(detect_shell_type(PathBuf::from("other")), None);
        assert_eq!(
            detect_shell_type(PathBuf::from("/bin/zsh")),
            Some(ShellType::Zsh)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("/bin/bash")),
            Some(ShellType::Bash)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("/usr/bin/bash")),
            Some(ShellType::Bash)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("powershell.exe")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from(if cfg!(windows) {
                "C:\\windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
            } else {
                "/usr/local/bin/pwsh"
            })),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("pwsh.exe")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("/usr/local/bin/pwsh")),
            Some(ShellType::PowerShell)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("/bin/sh")),
            Some(ShellType::Sh)
        );
        assert_eq!(detect_shell_type(PathBuf::from("sh")), Some(ShellType::Sh));
        assert_eq!(
            detect_shell_type(PathBuf::from("cmd")),
            Some(ShellType::Cmd)
        );
        assert_eq!(
            detect_shell_type(PathBuf::from("cmd.exe")),
            Some(ShellType::Cmd)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_is_wsl_bash_path() {
        assert!(is_wsl_bash_path(std::path::Path::new(
            r#"C:\Windows\System32\bash.exe"#
        )));
        assert!(is_wsl_bash_path(std::path::Path::new(
            r#"C:\Windows\Sysnative\bash.exe"#
        )));
        assert!(is_wsl_bash_path(std::path::Path::new(
            r#"C:\Windows\SysWOW64\bash.exe"#
        )));
        assert!(!is_wsl_bash_path(std::path::Path::new(
            r#"C:\Program Files\Git\bin\bash.exe"#
        )));
    }

    #[cfg(windows)]
    #[test]
    fn get_bash_shell_rejects_implicit_wsl_bash_lookup_results() {
        let detected_shell = get_bash_shell_with_resolvers(None, &|_| None, &|| {
            Some(PathBuf::from(r#"C:\Windows\System32\bash.exe"#))
        });

        assert_eq!(detected_shell, None);
    }

    #[cfg(windows)]
    #[test]
    #[serial(shell_env)]
    fn default_user_shell_prefers_bash_over_powershell_when_both_are_detectable() {
        let tempdir = tempdir().expect("tempdir");
        let bash_on_path = tempdir.path().join("bash.exe");
        let pwsh_on_path = tempdir.path().join("pwsh.exe");
        std::fs::write(&bash_on_path, b"").expect("write bash.exe");
        std::fs::write(&pwsh_on_path, b"").expect("write pwsh.exe");

        let temp_path = std::env::join_paths([tempdir.path()]).expect("join PATH");
        let _path_guard = EnvVarGuard::set("PATH", &temp_path);
        let _shell_override_guard = EnvVarGuard::remove("CODEX_SHELL_PATH");
        let _git_bash_override_guard = EnvVarGuard::remove("CODEX_GIT_BASH_PATH");

        let bash_shell = get_shell(ShellType::Bash, None).expect("expected Bash to be detectable");
        let powershell_shell =
            get_shell(ShellType::PowerShell, None).expect("expected PowerShell to be detectable");
        let default_shell = default_user_shell();

        assert_eq!(powershell_shell.shell_type, ShellType::PowerShell);
        assert_eq!(
            default_shell, bash_shell,
            "expected Git Bash to win when both Bash and PowerShell are detectable; got {:?}",
            default_shell.shell_path,
        );
    }

    #[test]
    #[serial(shell_env)]
    fn code_shell_path_override_takes_precedence_over_git_bash_override() {
        let tempdir = tempdir().expect("tempdir");
        let shell_override = tempdir.path().join("pwsh.exe");
        let git_bash_override = tempdir.path().join("bash.exe");
        std::fs::write(&shell_override, b"").expect("write shell override");
        std::fs::write(&git_bash_override, b"").expect("write git bash override");

        // SAFETY: this test only mutates process env for its own duration and
        // does not spawn concurrent threads that read these variables.
        unsafe {
            std::env::set_var("CODEX_SHELL_PATH", &shell_override);
            std::env::set_var("CODEX_GIT_BASH_PATH", &git_bash_override);
        }

        let override_path = shell_path_override_from_env();

        // SAFETY: paired cleanup for the env vars set above.
        unsafe {
            std::env::remove_var("CODEX_SHELL_PATH");
            std::env::remove_var("CODEX_GIT_BASH_PATH");
        }

        assert_eq!(override_path, Some(shell_override));
    }

    #[test]
    #[serial(shell_env)]
    fn code_git_bash_path_override_applies_when_shell_override_missing() {
        let tempdir = tempdir().expect("tempdir");
        let git_bash_override = tempdir.path().join("bash.exe");
        std::fs::write(&git_bash_override, b"").expect("write git bash override");

        // SAFETY: this test only mutates process env for its own duration and
        // does not spawn concurrent threads that read these variables.
        unsafe {
            std::env::remove_var("CODEX_SHELL_PATH");
            std::env::set_var("CODEX_GIT_BASH_PATH", &git_bash_override);
        }

        let override_path = shell_path_override_from_env();

        // SAFETY: paired cleanup for the env vars set above.
        unsafe {
            std::env::remove_var("CODEX_GIT_BASH_PATH");
        }

        assert_eq!(override_path, Some(git_bash_override));
    }
}
