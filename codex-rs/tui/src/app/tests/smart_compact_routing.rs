//! Pins the app-server method `/smart-compact` routes to.
//!
//! Everything else about the command is covered elsewhere: the chatwidget tests prove
//! `/smart-compact` emits `AppCommand::SmartCompact`, and the `codex-app-server` integration tests
//! prove `thread/smartCompact/start` drives the engine. Neither notices if the routing arm in
//! `thread_routing.rs` calls `thread_compact_start` instead, which would silently turn
//! `/smart-compact` into a whole-history `/compact` and delete the verbatim tail the feature
//! exists to keep. This test closes that gap by recording the JSON-RPC method actually sent.

use super::session_lifecycle_requests::start_recording_app_server;
use super::*;
use crate::app_command::AppCommand;
use codex_features::Feature;
use std::sync::Mutex;

const TEST_STACK_SIZE_BYTES: usize = 8 * 1024 * 1024;

fn recorded(requests: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    requests
        .lock()
        .expect("request recorder lock")
        .iter()
        .cloned()
        .collect()
}

#[test]
fn smart_compact_op_routes_to_the_smart_compact_method() -> Result<()> {
    std::thread::Builder::new()
        .name("tui-smart-compact-routing".to_string())
        .stack_size(TEST_STACK_SIZE_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut app = make_test_app().await;
                let codex_home = tempdir()?;
                app.config.codex_home = codex_home.path().to_path_buf().abs();
                app.config.sqlite_home = codex_home.path().to_path_buf();
                // The app-server refuses `thread/smartCompact/start` outright when the feature is
                // off, so it must be on or the routing arm is never reached. The thread resolves
                // its own features from `CODEX_HOME`, so writing config.toml is what actually
                // takes effect; the in-memory flag keeps the TUI-side dispatch consistent.
                std::fs::write(
                    codex_home.path().join("config.toml"),
                    "[features]
smart_compact = true
",
                )?;
                let _ = app.config.features.enable(Feature::SmartCompact);
                let (mut app_server, requests, proxy) =
                    start_recording_app_server(&app.config).await?;
                let mut tui = crate::tui::test_support::make_test_tui()?;

                app.start_fresh_session_with_summary_hint(
                    &mut tui,
                    &mut app_server,
                    /*session_start_source*/ None,
                    /*initial_user_message*/ None,
                    /*new_thread_name*/ None,
                )
                .await;
                let thread_id = app
                    .chat_widget
                    .thread_id()
                    .expect("fresh session should have a thread id");
                requests.lock().expect("request recorder lock").clear();

                app.submit_thread_op(&mut app_server, thread_id, AppCommand::SmartCompact)
                    .await?;

                let methods = recorded(&requests);
                assert!(
                    methods.iter().any(|m| m == "thread/smartCompact/start"),
                    "/smart-compact must route to thread/smartCompact/start, saw {methods:?}"
                );
                assert!(
                    !methods.iter().any(|m| m == "thread/compact/start"),
                    "/smart-compact must not fall back to whole-history compaction, saw {methods:?}"
                );

                app_server.shutdown().await?;
                proxy.await??;
                Ok(())
            })
        })?
        .join()
        .expect("smart compact routing test thread")
}

/// With the feature off the app server rejects the request, and the rejection must arrive as a
/// `TypedRequestError::Server` naming `thread/smartCompact/start`.
///
/// That exact shape is what `App::handle_event` matches on to recover instead of terminating the
/// TUI. If the error were reported differently, the recovery arm would silently stop matching and a
/// feature-flag mismatch against a remote app server would kill the session.
#[test]
fn smart_compact_rejection_is_a_typed_server_error_for_its_own_method() -> Result<()> {
    std::thread::Builder::new()
        .name("tui-smart-compact-rejection".to_string())
        .stack_size(TEST_STACK_SIZE_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut app = make_test_app().await;
                let codex_home = tempdir()?;
                app.config.codex_home = codex_home.path().to_path_buf().abs();
                app.config.sqlite_home = codex_home.path().to_path_buf();
                // Deliberately no `smart_compact` in config.toml: the feature defaults to off.
                let (mut app_server, _requests, proxy) =
                    start_recording_app_server(&app.config).await?;
                let mut tui = crate::tui::test_support::make_test_tui()?;

                app.start_fresh_session_with_summary_hint(
                    &mut tui,
                    &mut app_server,
                    /*session_start_source*/ None,
                    /*initial_user_message*/ None,
                    /*new_thread_name*/ None,
                )
                .await;
                let thread_id = app
                    .chat_widget
                    .thread_id()
                    .expect("fresh session should have a thread id");

                let err = app
                    .submit_thread_op(&mut app_server, thread_id, AppCommand::SmartCompact)
                    .await
                    .expect_err("a disabled feature must be rejected by the app server");
                match err.downcast_ref::<codex_app_server_client::TypedRequestError>() {
                    Some(codex_app_server_client::TypedRequestError::Server { method, .. }) => {
                        pretty_assertions::assert_eq!(method, "thread/smartCompact/start");
                    }
                    other => panic!("expected a typed server error, got {other:?}"),
                }
                assert!(
                    format!("{err:#}").contains("smart compaction is not enabled"),
                    "the rejection must carry readable prose, got {err:#}"
                );

                app_server.shutdown().await?;
                proxy.await??;
                Ok(())
            })
        })?
        .join()
        .expect("smart compact rejection test thread")
}

/// The whole flag-off TUI path, end to end through `App::handle_event`.
///
/// This is the test that actually pins the recovery arm in `event_dispatch.rs`: with the feature
/// off, the app server rejects `thread/smartCompact/start`, and `handle_event` must return
/// `AppRunControl::Continue` (not propagate the error, which terminates the TUI), render the
/// server's message, and clear the optimistic spinner.
#[test]
fn smart_compact_rejection_is_recovered_by_handle_event() -> Result<()> {
    std::thread::Builder::new()
        .name("tui-smart-compact-recovery".to_string())
        .stack_size(TEST_STACK_SIZE_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let (mut app, mut app_rx, _op_rx) = make_test_app_with_channels().await;
                let codex_home = tempdir()?;
                app.config.codex_home = codex_home.path().to_path_buf().abs();
                app.config.sqlite_home = codex_home.path().to_path_buf();
                // Deliberately no `smart_compact` in config.toml: the feature defaults to off.
                let (mut app_server, _requests, proxy) =
                    start_recording_app_server(&app.config).await?;
                let mut tui = crate::tui::test_support::make_test_tui()?;

                app.start_fresh_session_with_summary_hint(
                    &mut tui,
                    &mut app_server,
                    /*session_start_source*/ None,
                    /*initial_user_message*/ None,
                    /*new_thread_name*/ None,
                )
                .await;
                while app_rx.try_recv().is_ok() {}

                // Same optimistic state the slash dispatch sets before the request goes out.
                app.chat_widget.set_task_running_for_test(/*running*/ true);

                let control = Box::pin(app.handle_event(
                    &mut tui,
                    &mut app_server,
                    AppEvent::CodexOp(AppCommand::SmartCompact),
                ))
                .await?;

                assert!(
                    matches!(control, AppRunControl::Continue),
                    "a rejected smart compaction must not end the TUI session"
                );
                assert!(
                    !app.chat_widget.is_task_running_for_test(),
                    "the rejection must clear the optimistic spinner"
                );
                // The server's readable prose must actually reach the transcript. Without this an
                // empty or debug-formatted message would still satisfy the assertions above.
                let rendered = std::iter::from_fn(|| app_rx.try_recv().ok())
                    .filter_map(|event| match event {
                        AppEvent::InsertHistoryCell(cell) => Some(
                            cell.display_lines(/*width*/ 120)
                                .iter()
                                .map(|line| {
                                    line.spans
                                        .iter()
                                        .map(|span| span.content.clone())
                                        .collect::<String>()
                                })
                                .collect::<Vec<_>>()
                                .join(
                                    "
",
                                ),
                        ),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    );
                assert!(
                    rendered.contains("smart compaction is not enabled for this thread"),
                    "the server's reason must be rendered, got {rendered:?}"
                );
                // ...and as Display prose, not a debug dump of the error chain.
                for marker in [
                    "Caused by:",
                    "TypedRequestError",
                    "JSONRPCErrorError",
                    "code: ",
                ] {
                    assert!(
                        !rendered.contains(marker),
                        "the rendered refusal must not leak {marker:?}, got {rendered:?}"
                    );
                }

                app_server.shutdown().await?;
                proxy.await??;
                Ok(())
            })
        })?
        .join()
        .expect("smart compact recovery test thread")
}

/// The contrast case: `/compact` must keep routing to the stable whole-history method.
#[test]
fn compact_op_still_routes_to_the_plain_compact_method() -> Result<()> {
    std::thread::Builder::new()
        .name("tui-compact-routing".to_string())
        .stack_size(TEST_STACK_SIZE_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut app = make_test_app().await;
                let codex_home = tempdir()?;
                app.config.codex_home = codex_home.path().to_path_buf().abs();
                app.config.sqlite_home = codex_home.path().to_path_buf();
                // The app-server refuses `thread/smartCompact/start` outright when the feature is
                // off, so it must be on or the routing arm is never reached. The thread resolves
                // its own features from `CODEX_HOME`, so writing config.toml is what actually
                // takes effect; the in-memory flag keeps the TUI-side dispatch consistent.
                std::fs::write(
                    codex_home.path().join("config.toml"),
                    "[features]
smart_compact = true
",
                )?;
                let _ = app.config.features.enable(Feature::SmartCompact);
                let (mut app_server, requests, proxy) =
                    start_recording_app_server(&app.config).await?;
                let mut tui = crate::tui::test_support::make_test_tui()?;

                app.start_fresh_session_with_summary_hint(
                    &mut tui,
                    &mut app_server,
                    /*session_start_source*/ None,
                    /*initial_user_message*/ None,
                    /*new_thread_name*/ None,
                )
                .await;
                let thread_id = app
                    .chat_widget
                    .thread_id()
                    .expect("fresh session should have a thread id");
                requests.lock().expect("request recorder lock").clear();

                app.submit_thread_op(&mut app_server, thread_id, AppCommand::Compact)
                    .await?;

                let methods = recorded(&requests);
                assert!(
                    methods.iter().any(|m| m == "thread/compact/start"),
                    "/compact must route to thread/compact/start, saw {methods:?}"
                );
                assert!(
                    !methods.iter().any(|m| m == "thread/smartCompact/start"),
                    "/compact must not route to smart compaction, saw {methods:?}"
                );

                app_server.shutdown().await?;
                proxy.await??;
                Ok(())
            })
        })?
        .join()
        .expect("compact routing test thread")
}
