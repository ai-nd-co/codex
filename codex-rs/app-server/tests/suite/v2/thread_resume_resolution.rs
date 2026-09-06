use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_rollout;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::rollout_path;
use chrono::Utc;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;
use codex_state::StateRuntime;
use codex_state::ThreadMetadataBuilder;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[derive(Clone, Copy)]
enum MetadataPath {
    Missing,
    ForeignThread,
    Malformed,
    ArchivedMissing,
}

async fn resume_with_unusable_metadata_path(
    metadata_path: MetadataPath,
    keep_current_rollout: bool,
) -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let home = TempDir::new()?;
    let old_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model("gpt-6-astra")
        .write(home.path())?;
    let timestamp = "2025-01-05T12-00-00";
    let id = create_fake_rollout(
        home.path(),
        timestamp,
        "2025-01-05T12:00:00Z",
        "Current home history",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let current_path = rollout_path(home.path(), timestamp, &id);
    let cached_path = match metadata_path {
        MetadataPath::Missing | MetadataPath::ArchivedMissing => {
            rollout_path(old_home.path(), timestamp, &id)
        }
        MetadataPath::Malformed => {
            let path = rollout_path(old_home.path(), timestamp, &id);
            std::fs::create_dir_all(path.parent().expect("rollout parent"))?;
            std::fs::write(&path, "invalid session metadata\n")?;
            path
        }
        MetadataPath::ForeignThread => {
            let foreign_id = create_fake_rollout(
                old_home.path(),
                timestamp,
                "2025-01-05T12:00:00Z",
                "Foreign thread history",
                Some("mock_provider"),
                /*git_info*/ None,
            )?;
            rollout_path(old_home.path(), timestamp, &foreign_id)
        }
    };
    if !keep_current_rollout {
        std::fs::remove_file(&current_path)?;
    }
    let state = StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(home.path().abs()),
        "mock_provider".into(),
    )
    .await?;
    state
        .mark_backfill_complete(/*last_watermark*/ None)
        .await?;
    let builder = ThreadMetadataBuilder::new(
        ThreadId::from_string(&id)?,
        cached_path,
        Utc::now(),
        SessionSource::Cli,
    );
    let mut metadata = builder.build("mock_provider");
    metadata.cwd = home.path().to_path_buf();
    metadata.model = Some("gpt-5.4".to_string());
    metadata.reasoning_effort = Some(ReasoningEffort::High);
    if matches!(metadata_path, MetadataPath::ArchivedMissing) {
        metadata.archived_at = Some(Utc::now());
    }
    state.upsert_thread(&metadata).await?;

    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let request = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: id.clone(),
            cwd: Some(home.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await?;
    if keep_current_rollout && matches!(metadata_path, MetadataPath::Missing) {
        let response: ThreadResumeResponse = app.read_response(request).await?;
        assert_eq!(
            (
                response.thread.id,
                response.thread.preview,
                std::fs::canonicalize(response.thread.path.expect("resumed rollout"))?,
                response.model,
                response.reasoning_effort,
            ),
            (
                id,
                "Current home history".to_string(),
                std::fs::canonicalize(current_path)?,
                "gpt-5.4".to_string(),
                Some(ReasoningEffort::High),
            )
        );
    } else {
        let error = app
            .read_stream_until_error_message(codex_app_server_protocol::RequestId::Integer(request))
            .await?;
        let expected_code = match metadata_path {
            MetadataPath::Missing | MetadataPath::ArchivedMissing => -32600,
            MetadataPath::ForeignThread | MetadataPath::Malformed => -32603,
        };
        assert_eq!(error.error.code, expected_code);
        assert!(error.error.message.contains(&id), "{error:?}");
    }
    Ok(())
}

#[tokio::test]
async fn thread_resume_by_id_recovers_stale_home_path() -> Result<()> {
    resume_with_unusable_metadata_path(MetadataPath::Missing, /*keep_current_rollout*/ true).await
}

#[tokio::test]
async fn thread_resume_by_id_refuses_foreign_metadata_even_with_current_rollout() -> Result<()> {
    resume_with_unusable_metadata_path(
        MetadataPath::ForeignThread,
        /*keep_current_rollout*/ true,
    )
    .await
}

#[tokio::test]
async fn thread_resume_by_id_refuses_missing_rollout() -> Result<()> {
    resume_with_unusable_metadata_path(MetadataPath::Missing, /*keep_current_rollout*/ false).await
}

#[tokio::test]
async fn thread_resume_by_id_refuses_foreign_rollout() -> Result<()> {
    resume_with_unusable_metadata_path(
        MetadataPath::ForeignThread,
        /*keep_current_rollout*/ false,
    )
    .await
}

#[tokio::test]
async fn thread_resume_by_id_refuses_malformed_metadata_path() -> Result<()> {
    resume_with_unusable_metadata_path(MetadataPath::Malformed, /*keep_current_rollout*/ true).await
}

#[tokio::test]
async fn thread_resume_by_id_preserves_archived_metadata_refusal() -> Result<()> {
    resume_with_unusable_metadata_path(
        MetadataPath::ArchivedMissing,
        /*keep_current_rollout*/ true,
    )
    .await
}
