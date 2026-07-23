use super::*;
use crate::ConfigLayerStackOrdering;
use codex_file_system::CopyOptions;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystemFuture;
use codex_file_system::FileMetadata;
use codex_file_system::FileSystemReadStream;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::ReadDirectoryEntry;
use codex_file_system::RemoveOptions;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn config_layer_project_folders(stack: &ConfigLayerStack) -> Vec<String> {
    stack
        .get_layers(
            ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ true,
        )
        .into_iter()
        .filter_map(|layer| match &layer.name {
            ConfigLayerSource::Project { dot_codex_folder } => Some(
                dot_codex_folder
                    .as_path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("project folder name")
                    .to_string(),
            ),
            _ => None,
        })
        .collect()
}

struct TestFileSystem;

impl ExecutorFileSystem for TestFileSystem {
    fn canonicalize<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, PathUri> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            let canonicalized = path.canonicalize()?;
            Ok(PathUri::from_abs_path(&canonicalized))
        })
    }

    fn read_file<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            tokio::fs::read(path.as_path()).await
        })
    }

    fn read_file_stream<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileSystemReadStream> {
        Box::pin(async {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "test filesystem does not support streaming reads",
            ))
        })
    }

    fn write_file<'a>(
        &'a self,
        _path: &'a PathUri,
        _contents: Vec<u8>,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn create_directory<'a>(
        &'a self,
        _path: &'a PathUri,
        _create_directory_options: CreateDirectoryOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileMetadata> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            let metadata = std::fs::metadata(path.as_path())?;
            Ok(FileMetadata {
                is_directory: metadata.is_dir(),
                is_file: metadata.is_file(),
                is_symlink: metadata.file_type().is_symlink(),
                size: metadata.len(),
                created_at_ms: 0,
                modified_at_ms: 0,
            })
        })
    }

    fn read_directory<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<ReadDirectoryEntry>> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn remove<'a>(
        &'a self,
        _path: &'a PathUri,
        _remove_options: RemoveOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn copy<'a>(
        &'a self,
        _source_path: &'a PathUri,
        _destination_path: &'a PathUri,
        _copy_options: CopyOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }
}

#[tokio::test]
async fn profile_v2_rejects_matching_legacy_profile_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
model = "gpt-main"

[profiles.work]
model = "gpt-work"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    let err = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect_err("profile-v2 should reject a matching legacy profile in base user config");

    assert_eq!(
        err.kind(),
        io::ErrorKind::InvalidData,
        "a matching legacy profile should be a hard config error"
    );
    let message = err.to_string();
    assert!(
        message.contains("--profile `work` cannot be used"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("config.toml"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("[profiles.work]"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("https://developers.openai.com/codex/config-advanced#profiles"),
        "unexpected error message: {message}"
    );
}

#[tokio::test]
async fn profile_v2_rejects_matching_legacy_profile_selector_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
profile = "work"
model = "gpt-main"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    let err = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect_err("profile-v2 should reject a matching legacy profile selector");

    assert_eq!(
        err.kind(),
        io::ErrorKind::InvalidData,
        "a matching legacy profile selector should be a hard config error"
    );
    let message = err.to_string();
    assert!(
        message.contains("--profile `work` cannot be used"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("profile = \"work\""),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("work.config.toml"),
        "unexpected error message: {message}"
    );
}

#[tokio::test]
async fn profile_v2_allows_unrelated_legacy_profiles_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
model = "gpt-main"

[profiles.dev]
model = "gpt-dev"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("profile-v2 should allow unrelated legacy profiles in base user config");
}

#[tokio::test]
async fn project_layer_loading_scans_claude_and_codex_config_dirs() {
    let tmp = tempdir().expect("tempdir");
    let codex_home = tmp.path().join("home");
    let project_root = tmp.path().join("repo");
    let cwd = project_root.join("nested");

    std::fs::create_dir_all(&codex_home).expect("create codex_home");
    std::fs::create_dir_all(&cwd).expect("create cwd");
    std::fs::create_dir_all(project_root.join(".git")).expect("create .git");
    std::fs::create_dir_all(project_root.join(".claude")).expect("create .claude");
    std::fs::create_dir_all(project_root.join(".codex")).expect("create .codex");
    std::fs::write(project_root.join(".claude").join(CONFIG_TOML_FILE), "")
        .expect("write .claude config");
    std::fs::write(project_root.join(".codex").join(CONFIG_TOML_FILE), "")
        .expect("write .codex config");

    let stack = load_config_layers_state(
        &TestFileSystem,
        &codex_home,
        Some(AbsolutePathBuf::from_absolute_path(&cwd).expect("absolute cwd")),
        &[],
        LoaderOverrides::without_managed_config_for_tests(),
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("load config layers");

    assert_eq!(
        config_layer_project_folders(&stack),
        vec![".codex".to_string()]
    );
}

#[tokio::test]
async fn project_layer_loading_uses_claude_as_fallback_when_codex_absent() {
    let tmp = tempdir().expect("tempdir");
    let codex_home = tmp.path().join("home");
    let project_root = tmp.path().join("repo");
    let cwd = project_root.join("nested");

    std::fs::create_dir_all(&codex_home).expect("create codex_home");
    std::fs::create_dir_all(&cwd).expect("create cwd");
    std::fs::create_dir_all(project_root.join(".git")).expect("create .git");
    std::fs::create_dir_all(project_root.join(".claude")).expect("create .claude");
    std::fs::write(project_root.join(".claude").join(CONFIG_TOML_FILE), "")
        .expect("write .claude config");

    let stack = load_config_layers_state(
        &TestFileSystem,
        &codex_home,
        Some(AbsolutePathBuf::from_absolute_path(&cwd).expect("absolute cwd")),
        &[],
        LoaderOverrides::without_managed_config_for_tests(),
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("load config layers");

    assert_eq!(
        config_layer_project_folders(&stack),
        vec![".claude".to_string()]
    );
}

#[test]
fn linked_worktree_hook_override_preserves_active_project_folder_name() {
    let checkout_root =
        AbsolutePathBuf::from_absolute_path("/tmp/worktrees/repo").expect("checkout root");
    let repo_root = AbsolutePathBuf::from_absolute_path("/tmp/root/repo").expect("repo root");
    let config_file =
        AbsolutePathBuf::from_absolute_path("/tmp/home/config.toml").expect("user config");

    let context = ProjectTrustContext {
        project_root: checkout_root.clone(),
        project_root_key: "repo".to_string(),
        project_root_lookup_keys: vec!["repo".to_string()],
        checkout_root: Some(checkout_root.clone()),
        repo_root: Some(repo_root.clone()),
        repo_root_key: Some("repo".to_string()),
        repo_root_lookup_keys: Some(vec!["repo".to_string()]),
        projects_trust: std::collections::HashMap::new(),
        user_config_file: config_file,
    };

    let nested = checkout_root.join("subdir");
    assert_eq!(
        context.root_checkout_hooks_folder_for_dir(&nested, ".claude"),
        Some(repo_root.join("subdir").join(".claude"))
    );
    assert_eq!(
        context.root_checkout_hooks_folder_for_dir(&nested, ".codex"),
        Some(repo_root.join("subdir").join(".codex"))
    );
}
