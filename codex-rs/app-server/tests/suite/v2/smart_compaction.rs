//! End-to-end tests for `thread/smartCompact/start`, the selective-compaction invocation surface.
//!
//! Deliberately a separate module from `v2/compaction.rs` rather than an addition to it. This is a
//! fork-local feature in a hot upstream file; keeping it out of `compaction.rs` means the vendored
//! module stays byte-identical to upstream and the whole feature's test surface merges as an added
//! file. The few small helpers below are duplicated from `compaction.rs` for the same reason: a
//! visibility change there would be an upstream edit, and these are four-line wrappers.

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadSmartCompactStartParams;
use codex_app_server_protocol::ThreadSmartCompactStartResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_app_server_protocol::WarningNotification;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::TempDir;
use tokio::time::timeout;

#[cfg(any(target_os = "macos", windows))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

const COMPACT_PROMPT: &str = "Summarize the conversation.";
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;
/// Far above anything these tests produce: automatic compaction must never fire, or it would
/// replace the whole history and mask what smart compaction did.
const NO_AUTO_COMPACT: i64 = 100_000_000;

/// Text that shows up verbatim in a request body, long enough that the older half outweighs the
/// summary and the non-shrinking guard in `compact_smart` is satisfied.
fn bulky_reply(marker: &str) -> String {
    format!("{marker} ").repeat(400)
}

/// The invocation-surface happy path: `thread/smartCompact/start` reaches the engine, the older
/// turns are replaced by a summary, and the recent turns survive verbatim into the next request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_smart_compact_start_summarizes_older_turns_and_keeps_recent_turns_verbatim()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let turn1 = responses::sse(vec![
        responses::ev_assistant_message("m1", &bulky_reply("TURN_ONE_REPLY")),
        responses::ev_completed_with_tokens("r1", /*total_tokens*/ 100),
    ]);
    let turn2 = responses::sse(vec![
        responses::ev_assistant_message("m2", &bulky_reply("TURN_TWO_REPLY")),
        responses::ev_completed_with_tokens("r2", /*total_tokens*/ 200),
    ]);
    let turn3 = responses::sse(vec![
        responses::ev_assistant_message("m3", "TURN_THREE_REPLY"),
        responses::ev_completed_with_tokens("r3", /*total_tokens*/ 300),
    ]);
    let summarization = responses::sse(vec![
        responses::ev_assistant_message("m4", "SMART_SUMMARY_OF_OLDER_HALF"),
        responses::ev_completed_with_tokens("r4", /*total_tokens*/ 50),
    ]);
    let turn4 = responses::sse(vec![
        responses::ev_assistant_message("m5", "TURN_FOUR_REPLY"),
        responses::ev_completed_with_tokens("r5", /*total_tokens*/ 400),
    ]);
    let mock =
        responses::mount_sse_sequence(&server, vec![turn1, turn2, turn3, summarization, turn4])
            .await;

    let (mut mcp, _codex_home) =
        start_server(&server.uri(), /*smart_compact*/ true, NO_AUTO_COMPACT).await?;
    let thread_id = start_thread(&mut mcp).await?;

    send_turn_and_wait(&mut mcp, &thread_id, "TURN_ONE_QUESTION").await?;
    send_turn_and_wait(&mut mcp, &thread_id, "TURN_TWO_QUESTION").await?;
    send_turn_and_wait(&mut mcp, &thread_id, "TURN_THREE_QUESTION").await?;

    smart_compact(&mut mcp, &thread_id).await?;

    // Wait for the user-visible outcome first. On success that is a `warning` carrying the summary
    // line; on any refusal it is an `error` carrying the reason. Waiting for the compaction item
    // directly would simply hang on a refusal and hide which guard fired.
    let outcome = wait_for_smart_compact_outcome(&mut mcp).await?;
    assert!(
        outcome.starts_with("Smart compact: summarized the oldest "),
        "expected a successful smart-compact summary line, got {outcome:?}"
    );

    // The compaction must also *complete* as a turn item, not merely announce itself: an unfinished
    // compaction leaves a client spinner running forever.
    let started = wait_for_context_compaction(&mut mcp, "item/started").await?;
    let completed = wait_for_context_compaction(&mut mcp, "item/completed").await?;
    assert!(
        !started.is_empty(),
        "the context-compaction item must carry an id"
    );
    assert_eq!(
        started, completed,
        "the started and completed context-compaction item must be the same item"
    );

    let summary_request = mock
        .requests()
        .get(3)
        .cloned()
        .expect("summarization request");
    assert!(
        summary_request.body_contains_text("TURN_ONE_REPLY"),
        "the summarizer must see the older half"
    );
    assert!(
        !summary_request.body_contains_text("TURN_THREE_REPLY"),
        "the summarizer must not see the verbatim tail"
    );

    // The decisive assertion: the next real request shows the summary in place of the older turns
    // while the recent turns are still there byte for byte.
    send_turn_and_wait(&mut mcp, &thread_id, "TURN_FOUR_QUESTION").await?;
    let next_request = mock.requests().last().cloned().expect("post-compact turn");
    assert!(
        next_request.body_contains_text("SMART_SUMMARY_OF_OLDER_HALF"),
        "summary of the older half must be in the compacted history"
    );
    assert!(
        !next_request.body_contains_text("TURN_ONE_REPLY"),
        "the older half must have been replaced by the summary"
    );
    assert!(
        next_request.body_contains_text(&bulky_reply("TURN_TWO_REPLY")),
        "the newer half must survive verbatim"
    );
    assert!(
        next_request.body_contains_text("TURN_THREE_REPLY"),
        "the newer half must survive verbatim"
    );

    Ok(())
}

/// Flag off is the default. The refusal must be a readable message, not a panic, not a silent
/// no-op, and it must not cost a model request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_smart_compact_start_refuses_when_feature_is_disabled() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_assistant_message("m1", "FIRST_REPLY"),
                responses::ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("m2", "SECOND_REPLY"),
                responses::ev_completed_with_tokens("r2", /*total_tokens*/ 200),
            ]),
        ],
    )
    .await;

    // `smart_compact` is left out of the config entirely, so the feature's own
    // `default_enabled: false` applies.
    let (mut mcp, _codex_home) =
        start_server(&server.uri(), /*smart_compact*/ false, NO_AUTO_COMPACT).await?;
    let thread_id = start_thread(&mut mcp).await?;
    send_turn_and_wait(&mut mcp, &thread_id, "FIRST_QUESTION").await?;
    let requests_before = mock.requests().len();

    // The refusal comes back on the request's own channel, which cannot be lost. Relying on the
    // notification path instead was shown to drop the refusal entirely against the real binary.
    let request_id = mcp
        .send_thread_smart_compact_start_request(ThreadSmartCompactStartParams {
            thread_id: thread_id.clone(),
        })
        .await?;
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert_eq!(
        error.error.message,
        "smart compaction is not enabled for this thread; enable the `smart_compact` feature to use it"
    );
    assert_eq!(
        mock.requests().len(),
        requests_before,
        "a disabled flag must not cost a model request"
    );

    // A refusal must not poison the thread. Refusing before the op is submitted keeps
    // `ThreadState::turn_summary.last_error` untouched; an out-of-turn `Error` event stored there is
    // only cleared by a turn completing, so it would make the *next* ordinary turn report as failed.
    let next = send_turn_and_wait(&mut mcp, &thread_id, "SECOND_QUESTION").await?;
    assert_eq!(
        next.turn.error, None,
        "the next turn must not inherit the refusal error"
    );
    assert_eq!(
        next.turn.status,
        TurnStatus::Completed,
        "the next turn must complete normally after a refusal"
    );

    Ok(())
}

/// A `SplitRejection` must reach the client as prose, never as a debug-formatted enum.
///
/// One completed turn gives exactly one turn boundary, and everything above it is reinjected
/// context, so there is no interior boundary to cut at: `SplitRejection::NoInteriorTurnBoundary`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_smart_compact_start_explains_a_split_rejection_in_prose() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![responses::sse(vec![
            responses::ev_assistant_message("m1", "ONLY_REPLY"),
            responses::ev_completed_with_tokens("r1", /*total_tokens*/ 100),
        ])],
    )
    .await;

    let (mut mcp, _codex_home) =
        start_server(&server.uri(), /*smart_compact*/ true, NO_AUTO_COMPACT).await?;
    let thread_id = start_thread(&mut mcp).await?;
    send_turn_and_wait(&mut mcp, &thread_id, "ONLY_QUESTION").await?;
    let requests_before = mock.requests().len();

    smart_compact(&mut mcp, &thread_id).await?;

    let outcome = wait_for_smart_compact_outcome(&mut mcp).await?;
    assert_eq!(
        outcome,
        "Smart compact needs at least two turns so one half can be summarized while the other \
         stays verbatim; this thread has 1."
    );
    assert!(
        !outcome.contains("NoInteriorTurnBoundary"),
        "refusal must not leak the enum variant name"
    );
    assert_eq!(
        mock.requests().len(),
        requests_before,
        "a split rejection is decided before any model request"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_smart_compact_start_rejects_unknown_thread_id() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let (mut mcp, _codex_home) =
        start_server(&server.uri(), /*smart_compact*/ true, NO_AUTO_COMPACT).await?;

    let request_id = mcp
        .send_thread_smart_compact_start_request(ThreadSmartCompactStartParams {
            thread_id: "67e55044-10b1-426f-9247-bb680e5fe0c8".to_string(),
        })
        .await?;
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert!(error.error.message.contains("thread not found"));

    Ok(())
}

// ------------------------------------------------------------------ helpers

/// Returns the `TempDir` alongside the server: it must stay alive for the whole test, because the
/// server process reads `CODEX_HOME` from it.
async fn start_server(
    server_uri: &str,
    smart_compact: bool,
    auto_compact_limit: i64,
) -> Result<(TestAppServer, TempDir)> {
    let codex_home = TempDir::new()?;
    let features = if smart_compact {
        BTreeMap::from([(Feature::SmartCompact, true)])
    } else {
        BTreeMap::default()
    };
    write_mock_responses_config_toml(
        codex_home.path(),
        server_uri,
        &features,
        auto_compact_limit,
        /*requires_openai_auth*/ None,
        "mock_provider",
        COMPACT_PROMPT,
    )?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;
    Ok((mcp, codex_home))
}

/// Send `thread/smartCompact/start` and consume its acknowledgement.
async fn smart_compact(mcp: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let request_id = mcp
        .send_thread_smart_compact_start_request(ThreadSmartCompactStartParams {
            thread_id: thread_id.to_string(),
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let _: ThreadSmartCompactStartResponse =
        to_response::<ThreadSmartCompactStartResponse>(response)?;
    Ok(())
}

/// First user-visible smart-compact outcome: the success `warning` text or the refusal `error` text.
///
/// Returning either one is deliberate. A refusal must never be silent, so "no notification at all"
/// is a failure mode this helper surfaces as a timeout rather than hiding behind a wait for an event
/// that will never arrive.
async fn wait_for_smart_compact_outcome(mcp: &mut TestAppServer) -> Result<String> {
    loop {
        let notification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_matching_notification("warning or error", |notification| {
                notification.method == "warning" || notification.method == "error"
            }),
        )
        .await??;
        let params = notification.params.clone().expect("outcome params");
        let message = if notification.method == "warning" {
            serde_json::from_value::<WarningNotification>(params)?.message
        } else {
            serde_json::from_value::<ErrorNotification>(params)?
                .error
                .message
        };
        // Enabling an under-development feature emits its own startup warning, so filter on the
        // prefix every smart-compact message shares (success line and all refusal reasons alike).
        if message.starts_with("Smart compact") {
            return Ok(message);
        }
    }
}

/// Id of the next `contextCompaction` item seen on `method`.
async fn wait_for_context_compaction(mcp: &mut TestAppServer, method: &str) -> Result<String> {
    loop {
        let notification: JSONRPCNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_notification_message(method),
        )
        .await??;
        let params = notification.params.clone().expect("item params");
        let item = params.get("item").cloned().unwrap_or_default();
        if item.get("type").and_then(|value| value.as_str()) == Some("contextCompaction") {
            return Ok(item
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string());
        }
    }
}

async fn start_thread(mcp: &mut TestAppServer) -> Result<String> {
    let request_id = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(response)?;
    Ok(thread.id)
}

async fn send_turn_and_wait(
    mcp: &mut TestAppServer,
    thread_id: &str,
    text: &str,
) -> Result<TurnCompletedNotification> {
    let request_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response::<TurnStartResponse>(response)?;
    loop {
        let notification: JSONRPCNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        let completed: TurnCompletedNotification =
            serde_json::from_value(notification.params.clone().expect("turn/completed params"))?;
        if completed.turn.id == turn.id {
            return Ok(completed);
        }
    }
}
