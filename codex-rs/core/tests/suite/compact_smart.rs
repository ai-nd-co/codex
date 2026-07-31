//! Integration tests for selective ("smart") compaction: `Op::CompactSmart`.
//!
//! These drive a real `Session` against a mocked Responses endpoint, so they exercise the whole
//! path the feature actually uses: split selection, the local summarization turn, building the
//! replacement history, installing it through `replace_compacted_history`, and replaying it after
//! `resume`.

use anyhow::Result;
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_core::compact::SUMMARY_PREFIX;
use codex_core::config::Config;
use codex_features::Feature;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::protocol::Submission;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_reasoning_item;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::sse_failed;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::MockServer;

const TURN_ONE_USER: &str = "SMART_TURN_ONE_USER";
const TURN_TWO_USER: &str = "SMART_TURN_TWO_USER";
const TURN_THREE_USER: &str = "SMART_TURN_THREE_USER";
const TURN_FOUR_USER: &str = "SMART_TURN_FOUR_USER";
const TURN_ONE_REPLY: &str = "SMART_TURN_ONE_REPLY";
const TURN_TWO_REPLY: &str = "SMART_TURN_TWO_REPLY";
const TURN_THREE_REPLY: &str = "SMART_TURN_THREE_REPLY";
const TURN_FOUR_REPLY: &str = "SMART_TURN_FOUR_REPLY";
const SMART_SUMMARY_TEXT: &str = "SMART_SUMMARY_TEXT";
const AFTER_RESUME_USER: &str = "SMART_AFTER_RESUME_USER";
const AFTER_RESUME_REPLY: &str = "SMART_AFTER_RESUME_REPLY";
const STALE_ATTEMPT_TEXT: &str = "SMART_STALE_ATTEMPT_TEXT";

/// The message `handlers::compact_smart` sends when the feature flag is off.
const DISABLED_MESSAGE: &str =
    "Smart compact is not enabled. Enable the `smart_compact` feature to use it.";

/// Pad a marker so the conversation outweighs the session's initial-context bundle.
///
/// This is not cosmetic. The split target is token-weighted over the *whole* history, and a real
/// session opens with a developer instruction bundle that dwarfs a few short turns. With toy-sized
/// turns the token midpoint lands inside that bundle, the nearest boundary is the very first user
/// message, snapping it back over its pre-turn context collapses the cut to index 0, and the
/// selector correctly reports `NoSafeBoundary`. Padding puts the midpoint inside the conversation,
/// which is the regime the feature exists for.
fn padded(marker: &str) -> String {
    format!("{marker} {}", "lorem ipsum dolor sit amet ".repeat(400))
}

async fn start_conversation(
    server: &MockServer,
    smart_compact_enabled: bool,
) -> Result<(Arc<TempDir>, Config, Arc<ThreadManager>, Arc<CodexThread>)> {
    let test = Box::pin(
        test_codex()
            .with_config(move |config| {
                config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
                if smart_compact_enabled {
                    config
                        .features
                        .enable(Feature::SmartCompact)
                        .expect("test config should allow enabling smart_compact");
                }
            })
            .build(server),
    )
    .await?;
    Ok((test.home, test.config, test.thread_manager, test.codex))
}

async fn user_turn(conversation: &Arc<CodexThread>, text: &str) {
    conversation
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit user turn");
    wait_for_event(conversation, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}

/// What a smart-compact submission produced, up to and including its outcome message.
struct SmartCompactRun {
    message: String,
    /// A `ContextCompaction` `ItemStarted` **or** `ItemCompleted` was seen at all.
    ///
    /// A refusal must leave this false, and it deliberately covers a bare completion with no start:
    /// `ItemCompleted(ContextCompaction)` is not a neutral lifecycle marker - it maps to the legacy
    /// `ContextCompacted` success event and is persisted by the rollout policy - and
    /// `ContextCompactionItem` has no rejected status, so *any* compaction lifecycle event on a
    /// refusal publishes a durable claim that a compaction happened.
    saw_any_compaction_item_event: bool,
    /// A `ContextCompaction` item was started **and** the same item id was completed afterwards.
    compaction_item_completed: bool,
    /// The private summarization drain emitted its `RawResponseCompleted` parity event.
    saw_raw_response_completed: bool,
}

/// Submit `Op::CompactSmart` and drain events up to the outcome message.
///
/// Deliberately does not wait for `TurnComplete`: with the flag off no turn is spawned at all,
/// which is itself part of what the disabled path must guarantee.
///
/// The lifecycle bookkeeping matters: a refusal that emits `ItemStarted` without `ItemCompleted`
/// leaves clients showing a compaction that never finishes, and that failure is invisible to any
/// assertion that only looks at request bodies.
async fn smart_compact_run(conversation: &Arc<CodexThread>) -> SmartCompactRun {
    conversation
        .submit(Op::CompactSmart)
        .await
        .expect("submit smart compact");

    // The started item's id is remembered so completion is matched by identity *and* order:
    // completing a different compaction item, or completing before starting, leaves this false.
    let mut started_id: Option<String> = None;
    let mut completed = false;
    let mut saw_any_compaction_item_event = false;
    let mut saw_raw_response_completed = false;
    let message = loop {
        let event = wait_for_event(conversation, |ev| {
            matches!(
                ev,
                EventMsg::Warning(_)
                    | EventMsg::Error(_)
                    | EventMsg::ItemStarted(_)
                    | EventMsg::ItemCompleted(_)
                    | EventMsg::RawResponseCompleted(_)
            )
        })
        .await;
        match event {
            EventMsg::Warning(WarningEvent { message }) => break message,
            EventMsg::Error(ErrorEvent { message, .. }) => break message,
            EventMsg::RawResponseCompleted(_) => saw_raw_response_completed = true,
            EventMsg::ItemStarted(ev) => {
                if let TurnItem::ContextCompaction(item) = &ev.item {
                    saw_any_compaction_item_event = true;
                    started_id = Some(item.id.clone());
                }
            }
            EventMsg::ItemCompleted(ev) => {
                if let TurnItem::ContextCompaction(item) = &ev.item {
                    saw_any_compaction_item_event = true;
                    if started_id.as_deref() == Some(item.id.as_str()) {
                        completed = true;
                    }
                }
            }
            other => panic!("unexpected event {other:?}"),
        }
    };
    SmartCompactRun {
        message,
        saw_any_compaction_item_event,
        compaction_item_completed: completed,
        saw_raw_response_completed,
    }
}

async fn smart_compact(conversation: &Arc<CodexThread>) -> String {
    smart_compact_run(conversation).await.message
}

async fn wait_for_turn_complete(conversation: &Arc<CodexThread>) {
    wait_for_event(conversation, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}

fn three_turn_sse(summary: bool) -> Vec<String> {
    let mut bodies = vec![
        sse(vec![
            ev_assistant_message("m1", &padded(TURN_ONE_REPLY)),
            ev_completed("r1"),
        ]),
        sse(vec![
            ev_assistant_message("m2", &padded(TURN_TWO_REPLY)),
            ev_completed("r2"),
        ]),
        sse(vec![
            ev_assistant_message("m3", &padded(TURN_THREE_REPLY)),
            ev_completed("r3"),
        ]),
    ];
    if summary {
        bodies.push(sse(vec![
            ev_assistant_message("m4", SMART_SUMMARY_TEXT),
            ev_completed("r4"),
        ]));
    }
    bodies
}

async fn drive_three_turns(conversation: &Arc<CodexThread>) {
    user_turn(conversation, &padded(TURN_ONE_USER)).await;
    user_turn(conversation, &padded(TURN_TWO_USER)).await;
    user_turn(conversation, &padded(TURN_THREE_USER)).await;
}

fn expected_summary_text() -> String {
    format!("{SUMMARY_PREFIX}\n{SMART_SUMMARY_TEXT}")
}

/// Every marker-bearing message of the three-turn conversation, oldest first.
const CONVERSATION_MARKERS: [&str; 6] = [
    TURN_ONE_USER,
    TURN_ONE_REPLY,
    TURN_TWO_USER,
    TURN_TWO_REPLY,
    TURN_THREE_USER,
    TURN_THREE_REPLY,
];

/// Every occurrence of every marker in a request body, in body order.
///
/// Ordered, and counting **all** occurrences rather than the first: an omitted older item, an extra
/// newer item, a reordering, and a duplicated message all change this vector.
fn markers_in(
    request: &core_test_support::responses::ResponsesRequest,
    markers: &[&str],
) -> Vec<String> {
    let body = request.body_json().to_string();
    let mut found: Vec<(usize, String)> = Vec::new();
    for marker in markers {
        let mut from = 0usize;
        while let Some(at) = body[from..].find(marker) {
            let absolute = from + at;
            found.push((absolute, (*marker).to_string()));
            from = absolute + marker.len();
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, marker)| marker).collect()
}

fn as_strings(markers: &[&str]) -> Vec<String> {
    markers.iter().map(|m| (*m).to_string()).collect()
}

fn item_kind(item: &ResponseItem) -> String {
    match item {
        ResponseItem::Message { role, .. } => format!("message/{role}"),
        other => format!("{other:?}")
            .split_whitespace()
            .next()
            .unwrap_or("?")
            .to_string(),
    }
}

fn response_item_text(item: &ResponseItem) -> Option<String> {
    let ResponseItem::Message { content, .. } = item else {
        return None;
    };
    let mut text = String::new();
    for span in content {
        match span {
            ContentItem::InputText { text: part } | ContentItem::OutputText { text: part } => {
                text.push_str(part);
            }
            ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {}
        }
    }
    Some(text)
}

/// What one rollout file says about a compaction.
struct CompactionRollout {
    /// Response items persisted before the compaction checkpoint.
    recorded_before: Vec<ResponseItem>,
    /// Whether **any** `RolloutItem::Compacted` was written. Tracked separately from
    /// [`Self::replacement`] because a checkpoint without a replacement history is still replayed
    /// as a compaction, so "no replacement" is not the same as "no compaction happened".
    saw_checkpoint: bool,
    /// The installed replacement history, if a checkpoint carried one.
    replacement: Option<Vec<ResponseItem>>,
    /// `CompactedItem.message`, if a checkpoint was written at all.
    message: Option<String>,
}

/// A refusal must leave the durable record completely untouched: no compaction checkpoint of any
/// shape, and none of the summarizer's output persisted as ordinary history.
///
/// Both the raw model text and the prefixed summary form are searched. The mock emits the raw text,
/// while the installed item would carry `SUMMARY_PREFIX`, so checking only one of them would miss
/// half of what could leak.
fn assert_refusal_left_the_rollout_clean(path: &Path, what: &str) -> Result<()> {
    let rollout = compaction_rollout(path)?;
    assert!(
        !rollout.saw_checkpoint,
        "{what} must not persist a Compacted checkpoint of any shape"
    );
    for needle in [SUMMARY_PREFIX, SMART_SUMMARY_TEXT] {
        assert!(
            !rollout
                .recorded_before
                .iter()
                .any(|item| response_item_text(item).is_some_and(|t| t.contains(needle))),
            "{what} must not persist {needle:?} into ordinary history"
        );
    }
    Ok(())
}

fn compaction_rollout(path: &Path) -> Result<CompactionRollout> {
    let rollout_text = std::fs::read_to_string(path)?;
    let mut recorded_before = Vec::new();
    let mut saw_checkpoint = false;
    let mut replacement = None;
    let mut message = None;
    for line in rollout_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let entry: RolloutLine = serde_json::from_str(line)?;
        match entry.item {
            RolloutItem::ResponseItem(item) if !saw_checkpoint => recorded_before.push(item),
            RolloutItem::Compacted(compacted) => {
                saw_checkpoint = true;
                message = Some(compacted.message);
                if let Some(items) = compacted.replacement_history {
                    replacement = Some(items);
                }
            }
            _ => {}
        }
    }
    Ok(CompactionRollout {
        recorded_before,
        saw_checkpoint,
        replacement,
        message,
    })
}

/// With the flag off the op must change nothing: no model request, no history rewrite, and an
/// explicit refusal rather than a silent no-op or a fallback to `/compact`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_with_the_flag_off_changes_nothing() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ false);
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log: ResponseMock = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ false).await?;
    drive_three_turns(&conversation).await;
    let requests_before = request_log.requests().len();

    let message = smart_compact(&conversation).await;
    assert_eq!(message, DISABLED_MESSAGE);
    assert_eq!(
        request_log.requests().len(),
        requests_before,
        "a disabled smart compact must not issue a summarization request"
    );

    // The next turn proves history is untouched: every earlier turn is still present verbatim and
    // no compaction summary was inserted.
    user_turn(&conversation, &padded(TURN_FOUR_USER)).await;
    let requests = request_log.requests();
    let last = requests.last().expect("a request after the fourth turn");
    for text in [
        TURN_ONE_USER,
        TURN_ONE_REPLY,
        TURN_TWO_USER,
        TURN_TWO_REPLY,
        TURN_THREE_USER,
        TURN_THREE_REPLY,
        TURN_FOUR_USER,
    ] {
        assert!(
            last.body_contains_text(text),
            "history should be unchanged, but {text} is missing"
        );
    }
    assert!(
        !last.body_contains_text(SUMMARY_PREFIX),
        "no compaction summary should exist when the feature is disabled"
    );
    Ok(())
}

/// With the flag on: the summarizer sees only the older half, and the newer half survives verbatim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_summarizes_the_older_half_and_keeps_the_newer_half_verbatim() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ true);
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;

    let run = smart_compact_run(&conversation).await;
    assert!(
        run.message.starts_with("Smart compact:"),
        "expected a smart-compact outcome message, got {:?}",
        run.message
    );
    assert!(
        run.compaction_item_completed,
        "the ContextCompaction item must be started and completed"
    );
    wait_for_turn_complete(&conversation).await;

    let requests = request_log.requests();
    assert_eq!(requests.len(), 4, "three turns plus one summarization turn");
    let summarization = &requests[3];
    assert!(
        summarization.body_contains_text(SUMMARIZATION_PROMPT),
        "the summarization request should carry the compaction prompt"
    );

    // Exact, not "contains something expected": the summarizer input must be a strict, ordered
    // prefix of the conversation. Dropping an older item, leaking a newer one, or reordering all
    // change this vector.
    let summarizer_markers = markers_in(summarization, &CONVERSATION_MARKERS);
    assert!(
        !summarizer_markers.is_empty(),
        "the summarizer must see something to summarize"
    );
    assert!(
        summarizer_markers.len() < CONVERSATION_MARKERS.len(),
        "the summarizer must not see the whole conversation; that is /compact"
    );
    assert_eq!(
        summarizer_markers,
        as_strings(&CONVERSATION_MARKERS[..summarizer_markers.len()]),
        "the summarizer input must be an exact ordered prefix of the conversation"
    );

    user_turn(&conversation, &padded(TURN_FOUR_USER)).await;
    let requests = request_log.requests();
    let after = requests.last().expect("a request after the fourth turn");
    assert!(
        after.body_contains_text(&expected_summary_text()),
        "the compaction summary should be in the replacement history"
    );

    // The replacement history is exactly: retained *user* messages of the older half, then the
    // summary, then the newer half verbatim, then the new turn.
    let (older, newer) = CONVERSATION_MARKERS.split_at(summarizer_markers.len());
    let mut expected: Vec<String> = older
        .iter()
        .filter(|marker| marker.ends_with("USER"))
        .map(|marker| (*marker).to_string())
        .collect();
    expected.extend(as_strings(newer));
    expected.push(TURN_FOUR_USER.to_string());
    let mut all_markers = CONVERSATION_MARKERS.to_vec();
    all_markers.push(TURN_FOUR_USER);
    assert_eq!(
        markers_in(after, &all_markers),
        expected,
        "the post-compaction request must contain exactly the retained older user messages, then the verbatim newer half"
    );

    // Summary before the verbatim tail is the accepted deviation: assert the order explicitly so a
    // future change that silently reorders history fails here.
    let user_texts = after.message_input_texts("user");
    let summary_position = user_texts
        .iter()
        .position(|text| text.starts_with(SUMMARY_PREFIX))
        .expect("summary should be a user-role message");
    let tail_position = user_texts
        .iter()
        .position(|text| text.starts_with(TURN_THREE_USER))
        .expect("newer-half user message should still be present");
    assert!(
        summary_position < tail_position,
        "smart compact deliberately places the summary before the verbatim tail"
    );
    Ok(())
}

/// `codex resume` must replay the smart-compacted history plus everything appended after it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compacted_history_replays_after_resume() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ true);
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    bodies.push(sse(vec![
        ev_assistant_message("m6", &padded(AFTER_RESUME_REPLY)),
        ev_completed("r6"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, config, manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;
    let message = smart_compact(&conversation).await;
    assert!(
        message.starts_with("Smart compact:"),
        "expected a smart-compact outcome message, got {message:?}"
    );
    wait_for_turn_complete(&conversation).await;
    user_turn(&conversation, &padded(TURN_FOUR_USER)).await;

    let rollout_path = conversation.rollout_path().expect("rollout path");
    conversation
        .shutdown_and_wait()
        .await
        .expect("shutdown conversation");

    let auth_manager = codex_core::test_support::auth_manager_from_auth(
        codex_login::CodexAuth::from_api_key("dummy"),
    );
    let resumed = Box::pin(manager.resume_thread_from_rollout(
        config.clone(),
        rollout_path,
        auth_manager,
        /*parent_trace*/ None,
        /*supports_openai_form_elicitation*/ false,
    ))
    .await
    .expect("resume conversation")
    .thread;

    user_turn(&resumed, &padded(AFTER_RESUME_USER)).await;

    let requests = request_log.requests();
    let before_resume = &requests[requests.len() - 2];
    let after_resume = requests.last().expect("a request after resume");

    for text in [
        expected_summary_text().as_str(),
        TURN_THREE_USER,
        TURN_THREE_REPLY,
        TURN_FOUR_USER,
        TURN_FOUR_REPLY,
    ] {
        assert!(
            after_resume.body_contains_text(text),
            "resumed history should still contain {text}"
        );
    }
    assert!(
        !after_resume.body_contains_text(TURN_ONE_REPLY),
        "resume must replay the compacted history, not the pre-compaction one"
    );

    // The resumed request must extend the pre-resume one, not rebuild a different history.
    let before_user_texts = before_resume.message_input_texts("user");
    let after_user_texts = after_resume.message_input_texts("user");
    assert!(
        after_user_texts.starts_with(&before_user_texts),
        "after-resume user history should extend the pre-resume prefix\nbefore: {before_user_texts:?}\nafter:  {after_user_texts:?}"
    );
    assert!(
        after_user_texts
            .last()
            .is_some_and(|text| text.starts_with(AFTER_RESUME_USER)),
        "the resumed turn's own user message should be last"
    );
    Ok(())
}

/// Byte-identity at the **install** boundary, not just in the pure builder.
///
/// Reads the persisted `RolloutItem::Compacted { replacement_history }` and asserts that everything
/// after the summary is exactly the tail of the response items recorded before compaction. This is
/// the artifact `resume` replays, so proving it here proves the property survives persistence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn installed_replacement_history_keeps_the_newer_half_byte_identical() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let request_log = mount_sse_sequence(&server, three_turn_sse(/*summary*/ true)).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;
    let message = smart_compact(&conversation).await;
    assert!(
        message.starts_with("Smart compact:"),
        "expected a smart-compact outcome message, got {message:?}"
    );
    wait_for_turn_complete(&conversation).await;
    conversation.flush_rollout().await?;

    let rollout_path = conversation.rollout_path().expect("rollout path");
    let rollout = compaction_rollout(&rollout_path)?;
    let recorded_before = rollout.recorded_before;
    let replacement = rollout
        .replacement
        .expect("a compaction checkpoint should have been written");
    let compacted_message = rollout
        .message
        .expect("a compaction message should have been written");

    let summary_index = replacement
        .iter()
        .position(|item| response_item_text(item).is_some_and(|t| t.starts_with(SUMMARY_PREFIX)))
        .expect("replacement history should contain the compaction summary");
    // `CompactedItem.message` and the installed summary item must be the same string: they are
    // written from one value, and a future change that re-derives either one independently would
    // let the persisted metadata drift from what the model actually sees.
    assert_eq!(
        response_item_text(&replacement[summary_index]).as_deref(),
        Some(compacted_message.as_str()),
        "the persisted compaction message must equal the installed summary item"
    );
    let tail = &replacement[summary_index + 1..];
    assert!(
        !tail.is_empty(),
        "smart compact must keep a verbatim tail; an empty tail is whole-history compaction"
    );
    // The summarizer's raw output is collected locally and never recorded, so everything persisted
    // before the compaction line is the conversation itself. That makes the strongest form of the
    // assertion available: the tail must be the *literal suffix*. Accepting the tail anywhere in the
    // recorded items would pass a tail with items dropped off its front.
    assert!(
        !recorded_before
            .iter()
            .any(|item| response_item_text(item).is_some_and(|t| t.contains(SMART_SUMMARY_TEXT))),
        "the summarizer's raw response must never be persisted as an ordinary history item"
    );
    assert!(
        tail.len() < recorded_before.len(),
        "some of the conversation must actually have been compacted\ntail kinds: {:?}\nrecorded kinds: {:?}",
        tail.iter().map(item_kind).collect::<Vec<_>>(),
        recorded_before.iter().map(item_kind).collect::<Vec<_>>()
    );
    assert_eq!(
        tail,
        &recorded_before[recorded_before.len() - tail.len()..],
        "the verbatim tail must be the exact byte-identical suffix of the pre-compaction conversation"
    );

    // Exactness against the summarizer input: the summarization request must have seen precisely
    // the items the tail excludes, and nothing else. Comparing item counts catches dropped or
    // duplicated *unmarked* history, which a marker-based check cannot see.
    let split_index = recorded_before.len() - tail.len();
    let summarization = request_log
        .requests()
        .into_iter()
        .last()
        .expect("the summarization request");
    let input_len = summarization
        .body_json()
        .get("input")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .expect("summarization request should carry an input array");
    assert_eq!(
        input_len,
        split_index + 1,
        "the summarizer must see exactly the {split_index} older-half items plus the compaction prompt"
    );
    Ok(())
}

/// The context baseline is cleared (`InitialContextInjection::DoNotInject`), so the next regular
/// turn must reinject environment context in full rather than silently losing it with the older
/// half.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn environment_context_is_reinjected_after_smart_compaction() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ true);
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;
    smart_compact(&conversation).await;
    wait_for_turn_complete(&conversation).await;
    user_turn(&conversation, &padded(TURN_FOUR_USER)).await;

    let requests = request_log.requests();
    let first = &requests[0];
    let after = requests.last().expect("a request after compaction");

    let baseline_developer_texts = first.message_input_texts("developer");
    assert!(
        !baseline_developer_texts.is_empty(),
        "fixture should start with developer-role environment context"
    );
    let after_developer_texts = after.message_input_texts("developer");
    for text in &baseline_developer_texts {
        assert!(
            after_developer_texts.contains(text),
            "environment context should be reinjected after compaction, but this developer text is missing:\n{text}"
        );
    }
    Ok(())
}

/// A summarization turn that completes without an assistant message must NOT install an empty
/// summary. Doing so would delete the older half's assistant, reasoning and tool layer and replace
/// it with nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_refuses_when_the_model_returns_no_summary() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ false);
    // A response that completes but produces only reasoning: no assistant message to summarize
    // with, and an item that must not be left behind in history either.
    bodies.push(sse(vec![
        ev_reasoning_item("rs4", &[STALE_ATTEMPT_TEXT], &[STALE_ATTEMPT_TEXT]),
        ev_completed("r4"),
    ]));
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;

    let run = smart_compact_run(&conversation).await;
    assert!(
        run.message.contains("did not receive a summary"),
        "expected an explicit refusal, got {:?}",
        run.message
    );
    assert!(
        !run.saw_any_compaction_item_event,
        "a refusal must emit no ContextCompaction lifecycle event at all: any of them publishes a          durable success signal for a compaction that did not happen"
    );
    assert!(
        run.saw_raw_response_completed,
        "the private summarization drain must still emit RawResponseCompleted"
    );
    wait_for_turn_complete(&conversation).await;

    assert_history_untouched_by_refusal(&conversation, &request_log).await;
    Ok(())
}

/// Shared post-refusal check: the next real turn must see exactly the original conversation, with
/// no summary and no leftover summarizer output.
async fn assert_history_untouched_by_refusal(
    conversation: &Arc<CodexThread>,
    request_log: &ResponseMock,
) {
    user_turn(conversation, &padded(TURN_FOUR_USER)).await;
    let requests = request_log.requests();
    let after = requests.last().expect("a request after the fourth turn");
    let mut all_markers = CONVERSATION_MARKERS.to_vec();
    all_markers.push(TURN_FOUR_USER);
    assert_eq!(
        markers_in(after, &all_markers),
        as_strings(&all_markers),
        "a refused smart compact must leave the conversation exactly as it was"
    );
    assert!(
        !after.body_contains_text(SUMMARY_PREFIX),
        "no summary should have been installed"
    );
    assert!(
        !after.body_contains_text(STALE_ATTEMPT_TEXT),
        "the summarizer's own output must never enter the thread the user was told is unchanged"
    );
    assert!(
        !after.body_contains_text(SMART_SUMMARY_TEXT),
        "the summarizer's own output must never enter the thread the user was told is unchanged"
    );
}

/// A summary at least as large as the half it replaces would make the thread *bigger*. `/compact`
/// cannot hit this because it replaces the whole history; smart compact can, so it must refuse.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_refuses_when_it_would_not_reduce_context() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ false);
    // A "summary" far larger than the older half it would replace.
    let bloated = format!("{SMART_SUMMARY_TEXT} {}", "bloat ".repeat(40_000));
    bodies.push(sse(vec![
        ev_assistant_message("m4", &bloated),
        ev_completed("r4"),
    ]));
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;

    let message = smart_compact(&conversation).await;
    assert!(
        message.contains("would not reduce context"),
        "expected a non-shrinking refusal, got {message:?}"
    );
    wait_for_turn_complete(&conversation).await;

    assert_history_untouched_by_refusal(&conversation, &request_log).await;
    Ok(())
}

/// If the older half alone does not fit in the context window, `/compact`'s answer (drop the oldest
/// items and retry) is unsafe here: the summary would then cover only part of the older half while
/// installation still replaces all of it. Smart compact refuses instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_refuses_when_the_older_half_exceeds_the_context_window() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ false);
    bodies.push(sse_failed(
        "r4",
        "context_length_exceeded",
        "Your input exceeds the context window of this model. Please adjust your input and try again.",
    ));
    // The follow-up turn's response. Its very existence is the regression check: if this path ever
    // marks the context full again, the next turn compacts instead of answering and this body is
    // consumed by a summarization request rather than by turn four.
    bodies.push(sse(vec![
        ev_assistant_message("m5", &padded(TURN_FOUR_REPLY)),
        ev_completed("r5"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;

    let run = smart_compact_run(&conversation).await;
    assert!(
        run.message.contains("exceeded the context window"),
        "expected a context-window refusal, got {:?}",
        run.message
    );
    assert!(
        !run.saw_any_compaction_item_event,
        "a refusal must emit no ContextCompaction lifecycle event at all: any of them publishes a          durable success signal for a compaction that did not happen"
    );
    wait_for_turn_complete(&conversation).await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        4,
        "three turns plus the one failed summarization attempt, and no retry"
    );
    let summarization = requests.last().expect("the summarization attempt");
    let summarizer_markers = markers_in(summarization, &CONVERSATION_MARKERS);
    assert_eq!(
        summarizer_markers,
        as_strings(&CONVERSATION_MARKERS[..summarizer_markers.len()]),
        "even the failed attempt must have carried only the older half"
    );
    assert!(
        summarizer_markers.len() < CONVERSATION_MARKERS.len(),
        "the failed attempt must not have carried the whole conversation"
    );

    // The rollout is the durable record, so check it directly too: no compaction checkpoint may have
    // been written, and no summarizer output may have been persisted as ordinary history.
    conversation.flush_rollout().await?;
    let rollout_path = conversation.rollout_path().expect("rollout path");
    assert_refusal_left_the_rollout_clean(&rollout_path, "a refused smart compact")?;

    // The regression check for *not* marking the context full on this path. If it were marked full,
    // the next turn would auto-compact instead of answering, so its request would carry the
    // compaction prompt and would not carry the fourth user message.
    assert_history_untouched_by_refusal(&conversation, &request_log).await;
    let after = request_log
        .requests()
        .into_iter()
        .last()
        .expect("a request after the fourth turn");
    assert!(
        !after.body_contains_text(SUMMARIZATION_PROMPT),
        "the turn after this refusal must be an ordinary turn, not an auto-compaction: refusing \
         here must not mark the context full, which would discard the very tail smart compact \
         exists to preserve"
    );
    Ok(())
}

/// A failed summarization attempt must leave nothing behind: its output is collected locally, never
/// recorded into the session, so a retry's summary is the only one that can be installed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_summarization_attempt_leaves_nothing_in_history() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let mut bodies = three_turn_sse(/*summary*/ false);
    // Attempt 1 emits assistant prose and then dies before `response.completed`.
    bodies.push(sse(vec![ev_assistant_message("m4", STALE_ATTEMPT_TEXT)]));
    // Attempt 2 succeeds.
    bodies.push(sse(vec![
        ev_assistant_message("m5", SMART_SUMMARY_TEXT),
        ev_completed("r5"),
    ]));
    bodies.push(sse(vec![
        ev_assistant_message("m6", &padded(TURN_FOUR_REPLY)),
        ev_completed("r6"),
    ]));
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    drive_three_turns(&conversation).await;

    let message = smart_compact(&conversation).await;
    assert!(
        message.starts_with("Smart compact:"),
        "the retry should succeed, got {message:?}"
    );
    wait_for_turn_complete(&conversation).await;

    user_turn(&conversation, &padded(TURN_FOUR_USER)).await;
    let requests = request_log.requests();
    let after = requests.last().expect("a request after the fourth turn");
    assert!(
        after.body_contains_text(&expected_summary_text()),
        "the installed summary must come from the successful attempt"
    );
    assert!(
        !after.body_contains_text(STALE_ATTEMPT_TEXT),
        "the failed attempt's output must not survive anywhere in the thread"
    );
    assert!(
        after.body_contains_text(TURN_THREE_USER) && after.body_contains_text(TURN_THREE_REPLY),
        "the verbatim tail must be unaffected by the retry"
    );
    Ok(())
}

/// The per-item cap is enforceable end to end: when a compacted-half item cannot be brought under
/// it by truncating text, the whole compaction is refused and nothing is installed or persisted.
///
/// The oversized cost here is non-text: a turn is submitted with a gigantic submission id, which
/// core stamps onto every item it records as `turn_id`. `collect_user_messages` carries that
/// passthrough metadata into the retained user message, and `estimate_item_token_count` serializes
/// the whole item, so the rebuilt message is over the cap no matter how short its text becomes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_uncappable_compacted_item_refuses_the_whole_compaction() -> Result<()> {
    skip_if_no_network!(Ok(()));

    // Turns two and three are deliberately much heavier than turn one, so the token-weighted
    // midpoint lands well past turn one and turn one is certain to be in the older half.
    let heavy = |marker: &str| format!("{marker} {}", "lorem ipsum dolor sit amet ".repeat(3_000));

    let server = MockServer::start().await;
    let bodies = vec![
        sse(vec![
            ev_assistant_message("m1", TURN_ONE_REPLY),
            ev_completed("r1"),
        ]),
        sse(vec![
            ev_assistant_message("m2", &heavy(TURN_TWO_REPLY)),
            ev_completed("r2"),
        ]),
        sse(vec![
            ev_assistant_message("m3", &heavy(TURN_THREE_REPLY)),
            ev_completed("r3"),
        ]),
        sse(vec![
            ev_assistant_message("m4", SMART_SUMMARY_TEXT),
            ev_completed("r4"),
        ]),
    ];
    let request_log = mount_sse_sequence(&server, bodies).await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;

    // Turn one carries the oversized turn id, so it is the retained user message that blows the cap.
    // 60_000 bytes is comfortably past the 10_000-token cap under the four-bytes-per-token estimate,
    // and no amount of text truncation can bring the item back under it.
    conversation
        .submit_with_id(Submission {
            id: "z".repeat(60_000),
            op: Op::UserInput {
                items: vec![UserInput::Text {
                    text: TURN_ONE_USER.to_string(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                responsesapi_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            },
            client_user_message_id: None,
            trace: None,
        })
        .await
        .expect("submit oversized-id user turn");
    wait_for_turn_complete(&conversation).await;
    user_turn(&conversation, &heavy(TURN_TWO_USER)).await;
    user_turn(&conversation, &heavy(TURN_THREE_USER)).await;

    let run = smart_compact_run(&conversation).await;
    assert!(
        run.message.contains("per-item limit"),
        "expected the per-item cap refusal, got {:?}",
        run.message
    );
    assert!(
        !run.saw_any_compaction_item_event,
        "a refusal must emit no ContextCompaction lifecycle event at all: any of them publishes a          durable success signal for a compaction that did not happen"
    );
    wait_for_turn_complete(&conversation).await;

    conversation.flush_rollout().await?;
    let rollout_path = conversation.rollout_path().expect("rollout path");
    assert_refusal_left_the_rollout_clean(&rollout_path, "an over-cap refusal")?;
    let _ = &request_log;
    Ok(())
}

/// A history with no interior turn boundary cannot be split; the user must be told why instead of
/// getting a silent no-op or a panic.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smart_compact_reports_a_reason_when_no_split_exists() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let request_log = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_assistant_message("m1", &padded(TURN_ONE_REPLY)),
            ev_completed("r1"),
        ])],
    )
    .await;

    let (_home, _config, _manager, conversation) =
        start_conversation(&server, /*smart_compact_enabled*/ true).await?;
    user_turn(&conversation, &padded(TURN_ONE_USER)).await;
    let requests_before = request_log.requests().len();

    let message = smart_compact(&conversation).await;
    assert!(
        message.contains("at least two turns"),
        "expected the NoInteriorTurnBoundary explanation, got {message:?}"
    );
    assert_eq!(
        request_log.requests().len(),
        requests_before,
        "a rejected smart compact must not issue a summarization request"
    );
    wait_for_turn_complete(&conversation).await;
    Ok(())
}
