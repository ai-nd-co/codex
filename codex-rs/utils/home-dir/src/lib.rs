use codex_utils_absolute_path::AbsolutePathBuf;
use dirs::home_dir;
use std::path::Path;
use std::path::PathBuf;

pub const STATE_HOME_ENV: &str = "CODEX_STATE_HOME";

/// Returns the path to the Codex configuration directory, which can be
/// specified by the `CODEX_HOME` environment variable. If not set, defaults to
/// `~/.codex`.
///
/// - If `CODEX_HOME` is set, the value must exist and be a directory. The
///   value will be canonicalized and this function will Err otherwise.
/// - If `CODEX_HOME` is not set, this function does not verify that the
///   directory exists.
pub fn find_codex_home() -> std::io::Result<AbsolutePathBuf> {
    let codex_home_env = std::env::var("CODEX_HOME")
        .ok()
        .filter(|val| !val.is_empty());
    find_codex_home_from_env(codex_home_env.as_deref())
}

pub fn find_codex_state_home() -> std::io::Result<AbsolutePathBuf> {
    let cwd = std::env::current_dir()?;
    find_codex_state_home_with_cwd(&cwd)
}

pub fn find_codex_state_home_with_cwd(cwd: &Path) -> std::io::Result<AbsolutePathBuf> {
    let codex_home_env = std::env::var("CODEX_HOME")
        .ok()
        .filter(|val| !val.is_empty());
    let state_home_env = std::env::var(STATE_HOME_ENV)
        .ok()
        .filter(|val| !val.is_empty());
    find_codex_state_home_from_env(
        codex_home_env.as_deref(),
        state_home_env.as_deref(),
        cwd,
        home_dir(),
    )
}

fn find_codex_home_from_env(codex_home_env: Option<&str>) -> std::io::Result<AbsolutePathBuf> {
    // Honor the `CODEX_HOME` environment variable when it is set to allow users
    // (and tests) to override the default location.
    match codex_home_env {
        Some(val) => {
            let path = PathBuf::from(val);
            let metadata = std::fs::metadata(&path).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("CODEX_HOME points to {val:?}, but that path does not exist"),
                ),
                _ => std::io::Error::new(
                    err.kind(),
                    format!("failed to read CODEX_HOME {val:?}: {err}"),
                ),
            })?;

            if !metadata.is_dir() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("CODEX_HOME points to {val:?}, but that path is not a directory"),
                ))
            } else {
                let canonical = path.canonicalize().map_err(|err| {
                    std::io::Error::new(
                        err.kind(),
                        format!("failed to canonicalize CODEX_HOME {val:?}: {err}"),
                    )
                })?;
                AbsolutePathBuf::from_absolute_path(canonical)
            }
        }
        None => {
            let mut p = home_dir().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                )
            })?;
            p.push(".codex");
            AbsolutePathBuf::from_absolute_path(p)
        }
    }
}

pub fn is_project_local_codex_home(codex_home: &Path, cwd: &Path) -> bool {
    let Ok(codex_home) = canonicalize_existing_or_absolute(codex_home) else {
        return false;
    };
    let Ok(cwd) = AbsolutePathBuf::from_absolute_path(cwd) else {
        return false;
    };

    if cwd
        .ancestors()
        .map(|ancestor| canonicalize_existing_or_absolute(ancestor.join(".codex")))
        .filter_map(Result::ok)
        .any(|candidate| candidate == codex_home)
    {
        return true;
    }

    if codex_home
        .as_path()
        .file_name()
        .and_then(|name| name.to_str())
        != Some(".codex")
    {
        return false;
    }

    let Some(project_root) = codex_home.parent() else {
        return false;
    };
    project_root.join(".git").as_path().exists()
        || project_root.join(".jj").as_path().exists()
        || project_root.join("AGENTS.md").as_path().exists()
        || project_root.join("CODE.md").as_path().exists()
        || project_root.join("CLAUDE.md").as_path().exists()
}

fn canonicalize_existing_or_absolute(path: impl AsRef<Path>) -> std::io::Result<AbsolutePathBuf> {
    let path = path.as_ref();
    if path.exists() {
        let canonical = path.canonicalize()?;
        AbsolutePathBuf::from_absolute_path(canonical)
    } else {
        AbsolutePathBuf::from_absolute_path(path)
    }
}

fn find_codex_state_home_from_env(
    codex_home_env: Option<&str>,
    state_home_env: Option<&str>,
    cwd: &Path,
    user_home: Option<PathBuf>,
) -> std::io::Result<AbsolutePathBuf> {
    let codex_home = find_codex_home_from_env(codex_home_env)?;

    if let Some(raw) = state_home_env {
        return resolve_state_home_override(raw, cwd);
    }

    if is_project_local_codex_home(codex_home.as_path(), cwd) {
        let mut default_state_home = user_home.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find home directory for CODEX_STATE_HOME default",
            )
        })?;
        default_state_home.push(".codex");
        default_state_home.push("state");
        return AbsolutePathBuf::from_absolute_path(default_state_home);
    }

    Ok(codex_home)
}

fn resolve_state_home_override(raw: &str, cwd: &Path) -> std::io::Result<AbsolutePathBuf> {
    let path = AbsolutePathBuf::resolve_path_against_base(raw, cwd);
    match std::fs::metadata(path.as_path()) {
        Ok(metadata) if !metadata.is_dir() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{STATE_HOME_ENV} points to {raw:?}, but that path is not a directory"),
        )),
        Ok(_) | Err(_) => Ok(path),
    }
}

#[cfg(test)]
mod tests {
    use super::canonicalize_existing_or_absolute;
    use super::find_codex_home_from_env;
    use super::find_codex_state_home_from_env;
    use super::is_project_local_codex_home;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use dirs::home_dir;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn find_codex_home_env_missing_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let missing = temp_home.path().join("missing-codex-home");
        let missing_str = missing
            .to_str()
            .expect("missing codex home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(missing_str)).expect_err("missing CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.to_string().contains("CODEX_HOME"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_file_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let file_path = temp_home.path().join("codex-home.txt");
        fs::write(&file_path, "not a directory").expect("write temp file");
        let file_str = file_path
            .to_str()
            .expect("file codex home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(file_str)).expect_err("file CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_valid_directory_canonicalizes() {
        let temp_home = TempDir::new().expect("temp home");
        let temp_str = temp_home
            .path()
            .to_str()
            .expect("temp codex home path should be valid utf-8");

        let resolved = find_codex_home_from_env(Some(temp_str)).expect("valid CODEX_HOME");
        let expected = temp_home
            .path()
            .canonicalize()
            .expect("canonicalize temp home");
        let expected = AbsolutePathBuf::from_absolute_path(expected).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn find_codex_home_without_env_uses_default_home_dir() {
        let resolved =
            find_codex_home_from_env(/*codex_home_env*/ None).expect("default CODEX_HOME");
        let mut expected = home_dir().expect("home dir");
        expected.push(".codex");
        let expected = AbsolutePathBuf::from_absolute_path(expected).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn project_local_codex_home_detects_ancestor_dot_codex() {
        let repo = TempDir::new().expect("repo");
        let cwd = repo.path().join("nested/worktree");
        fs::create_dir_all(&cwd).expect("nested cwd");
        let codex_home = repo.path().join(".codex");
        fs::create_dir_all(&codex_home).expect(".codex");

        assert!(is_project_local_codex_home(&codex_home, &cwd));
    }

    #[test]
    fn project_local_codex_home_detects_repo_marker_parent() {
        let repo = TempDir::new().expect("repo");
        let cwd = TempDir::new().expect("cwd elsewhere");
        let codex_home = repo.path().join(".codex");
        fs::create_dir_all(&codex_home).expect(".codex");
        fs::write(repo.path().join("CLAUDE.md"), "hi").expect("marker");

        assert!(is_project_local_codex_home(&codex_home, cwd.path()));
    }

    #[test]
    fn state_home_defaults_to_global_state_for_project_local_codex_home() {
        let home = TempDir::new().expect("home");
        let repo = home.path().join("repo");
        let cwd = repo.join("nested");
        let codex_home = repo.join(".codex");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&codex_home).expect("codex home");

        let resolved = find_codex_state_home_from_env(
            Some(
                codex_home
                    .to_str()
                    .expect("project-local codex home should be valid utf-8"),
            ),
            None,
            &cwd,
            Some(home.path().to_path_buf()),
        )
        .expect("state home");

        let expected = canonicalize_existing_or_absolute(home.path().join(".codex/state"))
            .unwrap_or_else(|_| {
                AbsolutePathBuf::from_absolute_path(home.path().join(".codex/state"))
                    .expect("absolute state home")
            });
        assert_eq!(resolved, expected);
    }

    #[test]
    fn state_home_defaults_to_codex_home_for_non_project_local_home() {
        let home = TempDir::new().expect("home");
        let cwd = home.path().join("repo");
        let codex_home = home.path().join("global-codex-home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&codex_home).expect("codex home");

        let resolved = find_codex_state_home_from_env(
            Some(
                codex_home
                    .to_str()
                    .expect("global codex home should be valid utf-8"),
            ),
            None,
            &cwd,
            Some(home.path().to_path_buf()),
        )
        .expect("state home");

        let expected = canonicalize_existing_or_absolute(codex_home).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn state_home_override_wins_even_for_project_local_codex_home() {
        let home = TempDir::new().expect("home");
        let repo = home.path().join("repo");
        let cwd = repo.join("nested");
        let codex_home = repo.join(".codex");
        let state_home = home.path().join("custom-state-home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&codex_home).expect("codex home");

        let resolved = find_codex_state_home_from_env(
            Some(
                codex_home
                    .to_str()
                    .expect("project-local codex home should be valid utf-8"),
            ),
            Some(
                state_home
                    .to_str()
                    .expect("state home override should be valid utf-8"),
            ),
            &cwd,
            Some(home.path().to_path_buf()),
        )
        .expect("state home");

        let expected = AbsolutePathBuf::from_absolute_path(state_home).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn relative_state_home_override_resolves_against_cwd() {
        let home = TempDir::new().expect("home");
        let repo = home.path().join("repo");
        let cwd = repo.join("nested");
        let codex_home = repo.join(".codex");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&codex_home).expect("codex home");

        let resolved = find_codex_state_home_from_env(
            Some(
                codex_home
                    .to_str()
                    .expect("project-local codex home should be valid utf-8"),
            ),
            Some("../shared-state"),
            &cwd,
            Some(home.path().to_path_buf()),
        )
        .expect("state home");

        let expected =
            AbsolutePathBuf::from_absolute_path(PathBuf::from(&cwd).join("../shared-state"))
                .expect("absolute state home");
        assert_eq!(resolved, expected);
    }
}
