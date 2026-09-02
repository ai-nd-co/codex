use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml_with_chatgpt_base_url;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnEnqueueError;
use codex_app_server_protocol::TurnEnqueueErrorCode;
use codex_app_server_protocol::TurnEnqueueParams;
use codex_app_server_protocol::TurnEnqueueResponse;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::UserInput as V2UserInput;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

#[cfg(windows)]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);
#[cfg(not(windows))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn params(thread_id: &str, key: &str, text: &str) -> TurnEnqueueParams {
    TurnEnqueueParams {
        thread_id: thread_id.to_string(),
        idempotency_key: key.to_string(),
        client_user_message_id: Some(format!("client-{key}")),
        input: vec![V2UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        responsesapi_client_metadata: None,
        additional_context: None,
    }
}

#[tokio::test]
async fn turn_enqueue_runs_distinct_turns_fifo_and_deduplicates() -> Result<()> {
    let tmp = TempDir::new()?;
    let codex_home = tmp.path().join("codex_home");
    std::fs::create_dir(&codex_home)?;
    let server = create_mock_responses_server_repeating_assistant("done").await;
    write_mock_responses_config_toml_with_chatgpt_base_url(
        &codex_home,
        &server.uri(),
        &server.uri(),
    )?;

    let mut app = TestAppServer::builder()
        .with_codex_home(&codex_home)
        .without_managed_config()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app.initialize()).await??;
    let thread_request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(thread_request)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_response)?;

    let first_request = app
        .send_turn_enqueue_request(params(&thread.id, "one", "first"))
        .await?;
    let first_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(first_request)),
    )
    .await??;
    let first: TurnEnqueueResponse = to_response(first_response)?;
    assert!(!first.duplicate);

    let duplicate_request = app
        .send_turn_enqueue_request(params(&thread.id, "one", "first"))
        .await?;
    let duplicate_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(duplicate_request)),
    )
    .await??;
    let duplicate: TurnEnqueueResponse = to_response(duplicate_response)?;
    assert_eq!(
        duplicate,
        TurnEnqueueResponse {
            turn_id: first.turn_id.clone(),
            duplicate: true,
        }
    );

    let second_request = app
        .send_turn_enqueue_request(params(&thread.id, "two", "second"))
        .await?;
    let second_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(second_request)),
    )
    .await??;
    let second: TurnEnqueueResponse = to_response(second_response)?;
    assert_ne!(second.turn_id, first.turn_id);

    let conflict_request = app
        .send_turn_enqueue_request(params(&thread.id, "one", "changed"))
        .await?;
    let conflict: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(conflict_request)),
    )
    .await??;
    assert_eq!(
        serde_json::from_value::<TurnEnqueueError>(conflict.error.data.expect("typed data"))?,
        TurnEnqueueError {
            code: TurnEnqueueErrorCode::IdempotencyConflict,
        }
    );

    for expected_turn_id in [&first.turn_id, &second.turn_id] {
        let started: JSONRPCNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            app.read_stream_until_notification_message("turn/started"),
        )
        .await??;
        let started: TurnStartedNotification =
            serde_json::from_value(started.params.expect("started params"))?;
        assert_eq!(&started.turn.id, expected_turn_id);

        let completed: JSONRPCNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            app.read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        let completed: TurnCompletedNotification =
            serde_json::from_value(completed.params.expect("completed params"))?;
        assert_eq!(&completed.turn.id, expected_turn_id);
    }

    let requests = server.received_requests().await.expect("mock requests");
    let response_requests = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .collect::<Vec<_>>();
    assert_eq!(response_requests.len(), 2);
    let bodies = response_requests
        .iter()
        .map(|request| {
            request
                .body_json::<serde_json::Value>()
                .expect("response request body")["input"]
                .to_string()
        })
        .collect::<Vec<_>>();
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("first") && !body.contains("second"))
    );
    assert!(bodies.iter().any(|body| body.contains("second")));

    Ok(())
}

#[tokio::test]
async fn turn_enqueue_rejects_empty_identity_and_input_with_typed_errors() -> Result<()> {
    let tmp = TempDir::new()?;
    let codex_home = tmp.path().join("codex_home");
    std::fs::create_dir(&codex_home)?;
    let server = create_mock_responses_server_repeating_assistant("done").await;
    write_mock_responses_config_toml_with_chatgpt_base_url(
        &codex_home,
        &server.uri(),
        &server.uri(),
    )?;
    let mut app = TestAppServer::builder()
        .with_codex_home(&codex_home)
        .without_managed_config()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app.initialize()).await??;
    let thread_request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(thread_request)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_response)?;

    let cases = [
        (
            params(&thread.id, "", "text"),
            TurnEnqueueErrorCode::EmptyIdempotencyKey,
        ),
        (
            TurnEnqueueParams {
                input: Vec::new(),
                ..params(&thread.id, "empty-input", "unused")
            },
            TurnEnqueueErrorCode::EmptyInput,
        ),
    ];
    for (params, expected_code) in cases {
        let request = app.send_turn_enqueue_request(params).await?;
        let error: JSONRPCError = timeout(
            DEFAULT_READ_TIMEOUT,
            app.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        let data: TurnEnqueueError = serde_json::from_value(error.error.data.expect("typed data"))?;
        assert_eq!(data.code, expected_code);
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("mock requests")
            .is_empty()
    );
    Ok(())
}
