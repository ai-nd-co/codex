//! Selective ("smart") compaction: summarize the **older** part of the conversation while the
//! newer part stays verbatim.
//!
//! This is the production consumer of `context_manager::split`. The split index comes from
//! [`select_compaction_split`], which guarantees the cut is a completed-turn boundary that induces
//! no call/output closure violation on either side. Pre-existing violations (a `FunctionCall` whose
//! output never landed because the turn was interrupted) are a normal steady state and are
//! deliberately tolerated, not treated as a reason to refuse.
//!
//! # How this differs from `compact.rs`
//!
//! | | `/compact` (`compact::run_compact_task`) | smart compact (here) |
//! |---|---|---|
//! | summarizer input | the whole history | `items[..split]` only |
//! | replacement history | retained user messages + summary | retained user messages **of the older half** + summary + `items[split..]` verbatim |
//! | summary position | last | before the verbatim tail |
//!
//! # Local summarizer only
//!
//! Smart compact never routes through `compact_remote` / `compact_remote_v2`, even when the
//! provider advertises remote compaction (`compact::should_use_remote_compact_task`). Remote v2
//! returns an opaque encrypted `Compaction` item with `CompactedItem.message` persisted as an empty
//! string, so there would be no readable summary to place mid-history, and placing an opaque blob
//! there is unattested. `/compact` keeps its backend selection untouched.
//!
//! # Accepted deviation: the summary is not last
//!
//! `compact.rs` documents that the model is trained to see the compaction summary as the last item
//! in history *after mid-turn compaction* (see the `InitialContextInjection` doc comment). Keeping a
//! verbatim tail necessarily places the summary before it. That cost is accepted deliberately: the
//! alternative discards the tail, which is the entire feature. The feature flag contains the blast
//! radius while the deviation is measured separately.
//!
//! # Initial context (`InitialContextInjection::DoNotInject`)
//!
//! Smart compact is a standalone, user-initiated turn, exactly like manual `/compact`
//! (`compact::run_compact_task` passes `DoNotInject`). `DoNotInject` means the replacement history
//! carries no freshly built initial context and both baselines are cleared:
//! `replace_compacted_history` is called with `reference_context_item: None` and
//! `world_state_baseline: None`, and `ContextManager::replace` clears the world-state baseline
//! itself. A cleared `reference_context_item` makes the next regular turn emit a **full**
//! reinjection of context state rather than a diff, so environment context is restored on the next
//! turn instead of being lost.
//!
//! Reusing `compact::insert_initial_context_before_last_real_user_or_summary` would be actively
//! wrong here: it inserts immediately before the *last real user message*, which for smart compact
//! lives inside the verbatim tail. That would splice fresh context into the middle of the tail, far
//! below the summary, which is neither the trained placement nor a useful one.
//!
//! # Known limitation: reasoning items survive compaction here, and token accounting notices
//!
//! `compact_remote::should_keep_compacted_history_item` drops every `Reasoning` item, so after a
//! whole-history compaction none remain. Smart compact is the first path that leaves them in a
//! compacted history, because they are inside the verbatim tail and removing them would break the
//! byte-identity the feature promises.
//!
//! `ContextManager::get_total_token_usage` adds `get_non_last_reasoning_items_tokens()` when the
//! server has not reported reasoning tokens itself, so a reasoning-heavy tail can be counted twice:
//! once inside the recomputed estimate and again by that adjustment on the next turn. The visible
//! effect would be an automatic compaction firing sooner than the thread warrants, discarding the
//! tail smart compact just preserved. Fixing it belongs in the accounting layer, not here; the
//! feature flag is default off, and this is recorded for the follow-up that measures the feature.

use std::sync::Arc;

use crate::Prompt;
use crate::client::ModelClientSession;
use crate::client_common::ResponseEvent;
use crate::compact::CompactedHistoryMetadata;
use crate::compact::CompactionAnalyticsAttempt;
use crate::compact::CompactionAnalyticsDetails;
use crate::compact::InitialContextInjection;
use crate::compact::SUMMARIZATION_PROMPT;
use crate::compact::SUMMARY_PREFIX;
use crate::compact::build_compacted_history;
use crate::compact::build_compaction_initial_context;
use crate::compact::collect_user_messages;
use crate::compact::compaction_status_from_result;
use crate::context_manager::ContextManager;
use crate::context_manager::estimate_item_token_count;
use crate::context_manager::split::DEFAULT_SPLIT_TARGET_FRACTION;
use crate::context_manager::split::HistorySplit;
use crate::context_manager::split::SplitRejection;
use crate::context_manager::split::select_compaction_split;
use crate::event_mapping::is_contextual_dev_message_content;
use crate::event_mapping::is_contextual_user_message_content;
use crate::hook_runtime::PostCompactHookOutcome;
use crate::hook_runtime::PreCompactHookOutcome;
use crate::hook_runtime::run_post_compact_hooks;
use crate::hook_runtime::run_pre_compact_hooks;
use crate::responses_metadata::CodexResponsesMetadata;
use crate::responses_metadata::CodexResponsesRequestKind;
use crate::responses_metadata::CompactionTurnMetadata;
use crate::session::session::Session;
use crate::session::turn::get_last_assistant_message_from_turn;
use crate::session::turn_context::TurnContext;
use crate::util::backoff;
use codex_analytics::CompactionImplementation;
use codex_analytics::CompactionPhase;
use codex_analytics::CompactionReason;
use codex_analytics::CompactionStatus;
use codex_analytics::CompactionTrigger;
use codex_features::Feature;
use codex_protocol::ResponseItemId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RawResponseCompletedEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use codex_rollout_trace::InferenceTraceContext;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use futures::prelude::*;
use tracing::error;

/// Message shown when the op is submitted with the `smart_compact` feature flag off.
///
/// The flag is the containment boundary for the accepted "summary is not last" deviation, so a
/// disabled run must be an explicit, visible refusal rather than a silent fallback to `/compact`.
pub(crate) const SMART_COMPACT_DISABLED_MESSAGE: &str =
    "Smart compact is not enabled. Enable the `smart_compact` feature to use it.";

/// Prefix of the event emitted after a successful smart compaction.
pub(crate) const SMART_COMPACT_SUMMARY_PREFIX: &str = "Smart compact:";

/// The replacement history plus the two indices that make its shape assertable.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SmartCompactedHistory {
    pub(crate) items: Vec<ResponseItem>,
    /// Index of the compaction summary item inside [`Self::items`].
    pub(crate) summary_index: usize,
    /// Index inside [`Self::items`] where the verbatim newer half begins.
    ///
    /// Always `summary_index + 1`. `items[verbatim_start..]` is byte-identical to
    /// `input[split_index..]` as built here.
    ///
    /// **One transformation happens after this struct**, in the shared install path:
    /// `Session::replace_compacted_history` runs `assign_missing_response_item_ids` over the whole
    /// replacement history, which stamps a synthetic id onto any item that has none. Live history
    /// items always already carry one (`Session::prepare_conversation_items_for_history` assigns it
    /// at the recording boundary), so this is a no-op for a normal session. A history reconstructed
    /// from an older rollout that predates response-item ids can contain id-less items, and those
    /// gain an id here. That changes the item's id field only: no content, ordering, or membership
    /// change, and it is the same stamping every compaction has always applied.
    pub(crate) verbatim_start: usize,
}

/// Build the smart-compaction replacement history.
///
/// `= build_compacted_history(<user messages of items[..split_index]>, summary) ++ items[split_index..]`
///
/// **What survives from the older half.** The retained-user-message budget of the existing builder
/// (`compact::build_compacted_history`, `COMPACT_USER_MESSAGE_MAX_TOKENS`) is kept, scoped to the
/// older half only. Three reasons:
///
/// 1. It reuses the attested replacement builder rather than inventing a second shape. The
///    "summary is not last" deviation is already one departure from the trained layout; adding
///    "and the user's own words are gone too" stacks a second, unmeasured one.
/// 2. It cannot duplicate anything. The halves are disjoint by construction, so a retained message
///    from the older half never also appears in the verbatim tail.
/// 3. It is strictly a subset of what it replaces (possibly truncated), so the older half still
///    shrinks; only the appended summary adds tokens.
///
/// `collect_user_messages` already skips prior compaction summaries, so an older half that contains
/// an earlier `/compact` summary does not re-retain it; that content is instead covered by the new
/// summary, which was produced from the older half.
pub(crate) fn build_smart_compacted_history(
    items: &[ResponseItem],
    split_index: usize,
    summary_text: &str,
    turn_id: &str,
) -> SmartCompactedHistory {
    let split_index = split_index.min(items.len());
    let (older, newer) = items.split_at(split_index);

    let older_user_messages = collect_user_messages(older);
    let mut compacted = build_compacted_history(Vec::new(), &older_user_messages, summary_text);

    // `build_compacted_history` always appends the summary last, so it is the final element here.
    // It is the only item this compaction turn authored; every other item is either replayed from
    // the older half or carried verbatim, and already carries its own turn id.
    let summary_index = compacted.len().saturating_sub(1);
    if let Some(summary_item) = compacted.last_mut() {
        summary_item.set_turn_id_if_missing(turn_id);
    }
    // Assign ids here rather than leaving them to `Session::replace_compacted_history`. The cap
    // below is measured with `estimate_item_token_count`, which serializes the whole item, so an
    // id added *after* the check could push an item that just fit back over the limit. Assigning
    // first makes what is measured exactly what is installed, and makes the later pass a no-op.
    assign_missing_ids(&mut compacted);
    // Every item up to here is newly built by this compaction, so the per-item cap applies to all of
    // them, not just the summary. Runs before the tail is appended so the tail cannot be touched.
    cap_compacted_half_items(&mut compacted);
    let verbatim_start = compacted.len();
    compacted.extend_from_slice(newer);

    SmartCompactedHistory {
        items: compacted,
        summary_index,
        verbatim_start,
    }
}

/// Mirror of `Session::assign_missing_response_item_ids` for the compacted half only.
///
/// Duplicated deliberately, and it is only correct because it is idempotent with the session's own
/// pass: an item that already has a non-empty id is left alone there, so assigning here changes
/// nothing about what gets installed. See the call site for why the ordering matters.
fn assign_missing_ids(items: &mut [ResponseItem]) {
    for item in items {
        if item.id().is_some_and(|id| !id.is_empty()) {
            continue;
        }
        let Some(prefix) = item.id_prefix() else {
            continue;
        };
        item.set_id(Some(ResponseItemId::new(prefix)));
    }
}

/// Concatenated text of a message item, or `None` for anything that is not a message.
fn message_text(item: &ResponseItem) -> Option<String> {
    let ResponseItem::Message { content, .. } = item else {
        return None;
    };
    let mut text = String::new();
    for span in content {
        if let ContentItem::InputText { text: part } | ContentItem::OutputText { text: part } = span
        {
            text.push_str(part);
        }
    }
    Some(text)
}

/// The first compacted-half item that is still over the per-item cap, if any.
///
/// This is the enforceable postcondition behind `cap_compacted_half_items`. Truncating text cannot
/// rescue an item whose non-text cost alone exceeds the cap, so rather than installing it anyway
/// (or silently emptying it) the caller refuses the whole compaction.
fn oversized_compacted_item(items: &[ResponseItem]) -> Option<(usize, i64)> {
    items.iter().enumerate().find_map(|(index, item)| {
        let estimate = estimate_item_token_count(item);
        (estimate > SUMMARY_MAX_TOKENS as i64).then_some((index, estimate))
    })
}

/// Hard cap for the summary item, per `AGENTS.md` "Model visible context" rules 3 and 4
/// ("no unbounded items", "no items larger than 10K tokens").
const SUMMARY_MAX_TOKENS: usize = 10_000;

/// Budget handed to `truncate_text`, deliberately below [`SUMMARY_MAX_TOKENS`].
///
/// `truncate_text` appends an elision marker after truncating, and `approx_token_count` is a coarse
/// byte-based estimate, so truncating *at* the cap measures ~8 tokens over it. The margin makes the
/// cap hold under the same estimator the rest of the context accounting uses.
const SUMMARY_TRUNCATION_BUDGET_TOKENS: usize = 9_800;

// The margin is the whole point of having two constants; raising the budget to the cap reintroduces
// the overshoot the test `the_summary_item_is_hard_capped_at_ten_thousand_tokens` measured.
const _: () = assert!(SUMMARY_TRUNCATION_BUDGET_TOKENS < SUMMARY_MAX_TOKENS);

/// Build the summary item's text, bounded to [`SUMMARY_MAX_TOKENS`].
///
/// The cap is enforced here rather than assumed from the summarization prompt: the model is not
/// bound by a length request, and `AGENTS.md` ("Model visible context", rules 3 and 4) requires
/// every injected item to have a hard cap and to stay under 10K tokens. `ContextManager` cannot
/// enforce it for us, because `replace_compacted_history` installs history through
/// `ContextManager::replace`, which bypasses the `record_items` truncation path entirely.
///
/// **Known limitation:** the bound is measured with `approx_token_count`, the same four-bytes-per-
/// token estimate the rest of the context accounting uses (`COMPACT_USER_MESSAGE_MAX_TOKENS`,
/// `TruncationPolicy::Tokens`). Text that tokenizes worse than four bytes per token, emoji-dense
/// text in particular, can sit under the estimated budget while exceeding 10K real model tokens. A
/// byte-accurate cap would need a tokenizer that this crate does not have; using a different
/// estimator here than everywhere else would be worse.
pub(crate) fn build_summary_item_text(summary_suffix: &str) -> String {
    truncate_text(
        &format!("{SUMMARY_PREFIX}\n{summary_suffix}"),
        TruncationPolicy::Tokens(SUMMARY_TRUNCATION_BUDGET_TOKENS),
    )
}

/// Apply the per-item cap to freshly built compacted-half items.
///
/// `build_compacted_history` bounds retained user messages only in *aggregate*
/// (`COMPACT_USER_MESSAGE_MAX_TOKENS` = 20_000, `compact.rs`), so one 15K-token user message is
/// copied through intact. Those messages are new items this feature installs, so the per-item rule
/// applies to them exactly as it does to the summary.
///
/// Only the compacted half is passed in. The verbatim tail must never be touched: its items are
/// pre-existing history, already subject to whatever bounds applied when they were recorded, and
/// rewriting them would break the byte-identity the whole feature rests on.
///
/// The bound is checked against `estimate_item_token_count`, the whole-item estimator the context
/// layer uses everywhere else, rather than against each text span. A per-span check would miss
/// everything the item costs besides its own text.
///
/// An item already at or under the cap is left completely alone. That matters for the summary,
/// which `build_summary_item_text` has already truncated: re-truncating it here would replace its
/// omission marker with a second one and make the installed text differ from the
/// `CompactedItem.message` recorded alongside it.
fn cap_compacted_half_items(items: &mut [ResponseItem]) {
    // Successively harsher text budgets. The first that brings the *whole item* under the cap wins,
    // so an item whose non-text cost is large still ends up bounded, and the loop is finite.
    //
    // There is deliberately no zero budget: emptying a message cannot rescue an item whose non-text
    // cost alone exceeds the cap, and doing so would erase `SUMMARY_PREFIX` from the summary. That
    // case is caught by `oversized_compacted_item`, which refuses the compaction outright, so
    // nothing over the cap is ever installed.
    const BUDGETS: [usize; 4] = [
        SUMMARY_TRUNCATION_BUDGET_TOKENS,
        SUMMARY_TRUNCATION_BUDGET_TOKENS / 2,
        SUMMARY_TRUNCATION_BUDGET_TOKENS / 4,
        SUMMARY_TRUNCATION_BUDGET_TOKENS / 16,
    ];

    for item in items {
        if estimate_item_token_count(item) <= SUMMARY_MAX_TOKENS as i64 {
            continue;
        }
        // Nothing else in a compacted half carries free text; `build_compacted_history` only ever
        // emits messages.
        let original: Vec<String> = match item {
            ResponseItem::Message { content, .. } => content
                .iter()
                .map(|span| match span {
                    ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                        text.clone()
                    }
                    _ => String::new(),
                })
                .collect(),
            _ => continue,
        };
        for budget in BUDGETS {
            if let ResponseItem::Message { content, .. } = item {
                for (span, source) in content.iter_mut().zip(original.iter()) {
                    let (ContentItem::InputText { text } | ContentItem::OutputText { text }) = span
                    else {
                        continue;
                    };
                    *text = truncate_text(source, TruncationPolicy::Tokens(budget));
                }
            }
            if estimate_item_token_count(item) <= SUMMARY_MAX_TOKENS as i64 {
                break;
            }
        }
    }
}

/// Same coarse byte-based estimate the split selector targets, summed over a slice.
pub(crate) fn estimate_items_token_count(items: &[ResponseItem]) -> i64 {
    items
        .iter()
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add)
}

/// Token estimate of the part of a slice that compaction actually removes for more than one request.
///
/// **Contextual** material is excluded, because `DoNotInject` clears the context baseline and the
/// next regular turn reinjects it in full; counting it as "removed" would let the non-shrinking
/// guard credit a reduction that lasts exactly one request.
///
/// The exclusion is content-based, not role-based. Excluding every `developer` message would also
/// exclude historical developer items that are *not* reinjected - guardian-approval records,
/// interrupted-turn markers, the multi-agent usage hint - and if those dominate the older half the
/// guard would refuse a compaction that really does shrink persistent context.
/// `is_contextual_dev_message_content` is the same predicate rollback uses to decide what
/// `build_initial_context` will rebuild, and it is true for a mixed bundle too, which is correct
/// here: a cleared baseline reinjects the whole bundle, not just its contextual fragments.
fn estimate_replaceable_token_count(items: &[ResponseItem]) -> i64 {
    items
        .iter()
        .filter(|item| match item {
            ResponseItem::Message { role, content, .. } if role == "developer" => {
                !is_contextual_dev_message_content(content)
            }
            ResponseItem::Message { role, content, .. } if role == "user" => {
                !is_contextual_user_message_content(content)
            }
            _ => true,
        })
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add)
}

/// Stream one summarization attempt and return the items it produced.
///
/// Deliberately **not** `compact::drain_to_completed`. That helper records every streamed item into
/// the live session through `Session::record_conversation_items`, which also persists it to the
/// rollout. `/compact` can afford that because it reads its summary back out of history and then
/// replaces the whole thing. Smart compact cannot: it has refusal paths that run *after* a
/// successful stream, and any recorded output would then survive in a thread the user was told was
/// unchanged. Keeping the summarizer's output local makes "nothing was changed" literally true on
/// every path, including terminal stream errors.
/// `usage_applied` is set the moment this starts writing token usage into the session, **before**
/// the write itself, because `update_token_usage_info` can apply usage and *then* fail with
/// `SessionBudgetExceeded`. Callers use it to decide whether an error still needs accounting
/// restoration; assuming "error implies untouched" would be wrong for exactly that case.
async fn drain_summary_to_completed(
    sess: &Session,
    turn_context: &TurnContext,
    client_session: &mut ModelClientSession,
    responses_metadata: &CodexResponsesMetadata,
    prompt: &Prompt,
    usage_applied: &mut bool,
) -> CodexResult<Vec<ResponseItem>> {
    let mut stream = client_session
        .stream(
            prompt,
            &turn_context.model_info,
            &turn_context.session_telemetry,
            turn_context.reasoning_effort.clone(),
            turn_context.reasoning_summary,
            turn_context.config.service_tier.clone(),
            responses_metadata,
            // Matches `compact::drain_to_completed`: local compaction streams are untraced.
            &InferenceTraceContext::disabled(),
        )
        .await?;
    let mut produced = Vec::new();
    loop {
        let Some(event) = stream.next().await else {
            return Err(CodexErr::Stream(
                "stream closed before response.completed".into(),
                None,
            ));
        };
        match event {
            Ok(ResponseEvent::OutputItemDone(item)) => produced.push(item),
            Ok(ResponseEvent::ServerReasoningIncluded(included)) => {
                sess.set_server_reasoning_included(included).await;
            }
            Ok(ResponseEvent::RateLimits(snapshot)) => {
                sess.update_rate_limits(turn_context, snapshot).await;
            }
            Ok(ResponseEvent::Completed {
                response_id,
                token_usage,
                ..
            }) => {
                sess.send_event(
                    turn_context,
                    EventMsg::RawResponseCompleted(RawResponseCompletedEvent {
                        response_id,
                        token_usage: token_usage.clone(),
                    }),
                )
                .await;
                // Only when there is usage to apply: a completed response with no usage writes
                // nothing, and claiming otherwise would make a later refusal overwrite an
                // authoritative server value with the coarse local estimate for no reason. Set
                // before the call, because the call can apply usage and then fail.
                *usage_applied |= token_usage.is_some();
                sess.update_token_usage_info(turn_context, token_usage.as_ref())
                    .await?;
                return Ok(produced);
            }
            Ok(_) => continue,
            Err(e) => return Err(e),
        }
    }
}

/// A user-facing explanation for every [`SplitRejection`] variant.
///
/// Every variant is handled explicitly; there is no catch-all arm, so a new variant is a compile
/// error rather than a silent no-op.
pub(crate) fn describe_split_rejection(rejection: &SplitRejection) -> String {
    match rejection {
        SplitRejection::EmptyHistory => {
            "Smart compact needs some conversation history to work with; this thread has none yet."
                .to_string()
        }
        SplitRejection::NoTurnBoundary => {
            "Smart compact could not find a completed turn to split on. Send at least one message first.".to_string()
        }
        SplitRejection::NoInteriorTurnBoundary { boundaries } => format!(
            "Smart compact needs at least two turns so one half can be summarized while the other stays verbatim; this thread has {boundaries}."
        ),
        SplitRejection::NoSafeBoundary {
            candidates_tried,
            unusable_candidates,
            nearest_induced,
        } => {
            let induced = nearest_induced.len();
            format!(
                "Smart compact could not find a safe place to split this conversation ({candidates_tried} boundaries checked, {unusable_candidates} with nothing to compact before them, {induced} unpaired tool calls at the nearest one). Use /compact instead."
            )
        }
    }
}

/// Outcome of one smart-compaction attempt that did not fail with a stream error.
#[derive(Debug)]
enum SmartCompactOutcome {
    Compacted {
        older_items: usize,
        newer_items: usize,
        older_token_estimate: i64,
        newer_token_estimate: i64,
        preexisting_violations: usize,
    },
    /// Nothing was compacted and history is unchanged. Carries the message shown to the user.
    Rejected { message: String },
}

pub(crate) async fn run_smart_compact_task(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
) -> CodexResult<()> {
    // Defense in depth: `handlers::compact_smart` refuses before spawning a task, so reaching this
    // with the flag off would mean a new caller bypassed the gate.
    if !sess.enabled(Feature::SmartCompact) {
        send_smart_compact_error(
            &sess,
            &turn_context,
            SMART_COMPACT_DISABLED_MESSAGE.to_string(),
        )
        .await;
        return Ok(());
    }

    let start_event = EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_context.sub_id.clone(),
        trace_id: turn_context.trace_id.clone(),
        started_at: turn_context.turn_timing_state.started_at_unix_secs().await,
        model_context_window: turn_context.model_context_window(),
        collaboration_mode_kind: turn_context.mode,
    });
    sess.send_event(&turn_context, start_event).await;

    let trigger = CompactionTrigger::Manual;
    let reason = CompactionReason::UserRequested;
    let phase = CompactionPhase::StandaloneTurn;
    let attempt = CompactionAnalyticsAttempt::begin(
        sess.as_ref(),
        turn_context.as_ref(),
        trigger,
        reason,
        CompactionImplementation::Responses,
        phase,
    )
    .await;

    // Smart compact replaces model-visible context exactly like `/compact` does, so a configured
    // PreCompact hook must be able to block it and a PostCompact hook must still observe it.
    match run_pre_compact_hooks(&sess, &turn_context, trigger).await {
        PreCompactHookOutcome::Continue => {}
        PreCompactHookOutcome::Stopped => {
            let error = CodexErr::TurnAborted;
            attempt
                .track(
                    sess.as_ref(),
                    CompactionStatus::Interrupted,
                    Some(&error),
                    CompactionAnalyticsDetails::default(),
                )
                .await;
            return Err(error);
        }
    }

    let result = run_smart_compact_impl(
        Arc::clone(&sess),
        Arc::clone(&turn_context),
        CompactionTurnMetadata::new(trigger, reason, CompactionImplementation::Responses, phase),
    )
    .await;

    // A rejection is not a completed compaction: nothing was summarized and history is unchanged.
    // Reporting it as `Completed` would make the analytics claim a context reduction that never
    // happened, so it is tracked as `Failed` even though the task itself returns `Ok`.
    let status = match &result {
        Ok(SmartCompactOutcome::Rejected { .. }) => CompactionStatus::Failed,
        other => compaction_status_from_result(other),
    };
    let codex_error = result.as_ref().err();
    if let Ok(SmartCompactOutcome::Compacted { .. }) = &result
        && let PostCompactHookOutcome::Stopped =
            run_post_compact_hooks(&sess, &turn_context, trigger).await
    {
        attempt
            .track(
                sess.as_ref(),
                status,
                codex_error,
                CompactionAnalyticsDetails::default(),
            )
            .await;
        return Err(CodexErr::TurnAborted);
    }
    attempt
        .track(
            sess.as_ref(),
            status,
            codex_error,
            CompactionAnalyticsDetails::default(),
        )
        .await;

    match result {
        Ok(SmartCompactOutcome::Compacted {
            older_items,
            newer_items,
            older_token_estimate,
            newer_token_estimate,
            preexisting_violations,
        }) => {
            let mut message = format!(
                "{SMART_COMPACT_SUMMARY_PREFIX} summarized the oldest {older_items} history items (~{older_token_estimate} tokens) and kept the most recent {newer_items} items (~{newer_token_estimate} tokens) verbatim."
            );
            if preexisting_violations > 0 {
                // These were already in the history before the split; `for_prompt` handles them the
                // same way it does today. Reported so the count is not mistaken for damage the
                // split caused.
                message.push_str(&format!(
                    " {preexisting_violations} pre-existing unpaired tool calls were carried through unchanged."
                ));
            }
            sess.send_event(&turn_context, EventMsg::Warning(WarningEvent { message }))
                .await;
            Ok(())
        }
        Ok(SmartCompactOutcome::Rejected { message }) => {
            send_smart_compact_error(&sess, &turn_context, message).await;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

/// Undo the summarization request's effect on token accounting after a refusal.
///
/// A completed summarization writes its own usage into `last_token_usage`
/// (`drain_summary_to_completed` -> `Session::update_token_usage_info`), but on a refusal the live
/// history is unchanged, so leaving that in place makes the reported context size describe a request
/// that is not the thread. `recompute_token_usage` re-derives it from the actual history, which is
/// exactly what the success path already does after installing.
async fn restore_token_accounting(sess: &Arc<Session>, turn_context: &Arc<TurnContext>) {
    sess.recompute_token_usage(turn_context).await;
}

/// Restore only when the summarizer actually wrote usage.
///
/// Recomputing unconditionally would replace an authoritative server-reported `last_token_usage`
/// with the deliberately coarse local estimate on errors that never touched accounting at all.
async fn restore_token_accounting_if(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    usage_applied: bool,
) {
    if usage_applied {
        restore_token_accounting(sess, turn_context).await;
    }
}

async fn send_smart_compact_error(sess: &Session, turn_context: &TurnContext, message: String) {
    sess.send_event(
        turn_context,
        EventMsg::Error(ErrorEvent {
            message,
            codex_error_info: Some(CodexErrorInfo::Other),
        }),
    )
    .await;
}

async fn run_smart_compact_impl(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    compaction_metadata: CompactionTurnMetadata,
) -> CodexResult<SmartCompactOutcome> {
    let pre_turn_history = sess.clone_history().await;
    let pre_turn_items = pre_turn_history.raw_items().to_vec();

    let split: HistorySplit =
        match select_compaction_split(&pre_turn_items, DEFAULT_SPLIT_TARGET_FRACTION) {
            Ok(split) => split,
            Err(rejection) => {
                return Ok(SmartCompactOutcome::Rejected {
                    message: describe_split_rejection(&rejection),
                });
            }
        };

    // The summarizer sees the older half only, so the summary describes exactly what is being
    // replaced. `select_compaction_split` guarantees this slice is closed, which is what keeps
    // `for_prompt` from fabricating an `"aborted"` output the summarizer would then believe.
    let prompt_text = turn_context
        .config
        .compact_prompt
        .as_deref()
        .unwrap_or(SUMMARIZATION_PROMPT)
        .to_string();
    let prompt_input: ResponseInputItem = ResponseInputItem::from(vec![UserInput::Text {
        text: prompt_text,
        // Compaction prompt is synthesized; no UI element ranges to preserve.
        text_elements: Vec::new(),
    }]);
    let mut summarizer_history = pre_turn_history.clone();
    summarizer_history.replace(pre_turn_items[..split.split_index].to_vec());
    summarizer_history.record_items(
        &[prompt_input.into()],
        turn_context.model_info.truncation_policy.into(),
    );

    // The `ContextCompaction` turn item is started **inside** `summarize_and_install`, immediately
    // before the replacement history is installed, and completed immediately after.
    //
    // It is deliberately not started around the whole operation. `ItemCompleted(ContextCompaction)`
    // is not a neutral lifecycle marker: it maps to the legacy `ContextCompacted` success event
    // (`protocol/src/legacy_events.rs`) and is persisted by the rollout policy, and
    // `ContextCompactionItem` has no failed or rejected status. Starting it earlier would force a
    // choice between two wrong options on a refusal - completing it, which publishes a durable
    // "compacted" signal for a compaction that did not happen, or leaving it in progress forever.
    // Starting it only once the outcome is certain avoids both.
    //
    // No token-accounting restoration here either. `summarize_and_install` already restores on
    // exactly the paths that could have written usage (`update_token_usage_info` can apply and then
    // fail), and doing it again unconditionally would replace an authoritative server-reported
    // `last_token_usage` with the coarse local estimate on paths that never touched accounting.
    summarize_and_install(
        &sess,
        &turn_context,
        compaction_metadata,
        summarizer_history,
        &pre_turn_items,
        &split,
    )
    .await
}

/// Run the summarization turn and, if everything checks out, install the replacement history.
///
/// Owns the `ContextCompaction` turn item, which it starts only once installation is certain; see
/// the call site for why.
async fn summarize_and_install(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    compaction_metadata: CompactionTurnMetadata,
    summarizer_history: ContextManager,
    pre_turn_items: &[ResponseItem],
    split: &HistorySplit,
) -> CodexResult<SmartCompactOutcome> {
    let max_retries = turn_context.provider.info().stream_max_retries();
    let mut retries = 0;
    let mut client_session = sess.services.model_client.new_session();
    let window_id = sess.current_window_id().await;
    let responses_metadata = turn_context.turn_metadata_state.to_responses_metadata(
        sess.installation_id.clone(),
        window_id,
        CodexResponsesRequestKind::Compaction(compaction_metadata),
    );

    // `update_token_usage_info` can apply the summarizer request's usage and *then* fail (rollout
    // budget accounting raises `SessionBudgetExceeded` from inside it), so "errored" does not imply
    // "accounting untouched". Every error return below restores accounting when, and only when,
    // usage was actually written; history is unchanged on all of them.
    let mut usage_applied = false;
    let summarizer_output = loop {
        let turn_input = summarizer_history
            .clone()
            .for_prompt(&turn_context.model_info.input_modalities);
        let prompt = Prompt {
            input: turn_input,
            base_instructions: sess.get_base_instructions().await,
            ..Default::default()
        };
        match drain_summary_to_completed(
            sess,
            turn_context.as_ref(),
            &mut client_session,
            &responses_metadata,
            &prompt,
            &mut usage_applied,
        )
        .await
        {
            Ok(items) => break items,
            Err(err @ (CodexErr::Interrupted | CodexErr::TurnAborted)) => {
                restore_token_accounting_if(sess, turn_context, usage_applied).await;
                return Err(err);
            }
            Err(e @ CodexErr::SessionBudgetExceeded) => {
                restore_token_accounting_if(sess, turn_context, usage_applied).await;
                sess.track_turn_codex_error(turn_context.as_ref(), &e);
                let event = EventMsg::Error(e.to_error_event(/*message_prefix*/ None));
                sess.send_event(turn_context, event).await;
                return Err(e);
            }
            Err(CodexErr::ContextWindowExceeded) => {
                // `compact.rs` recovers here by dropping the oldest items and retrying. Smart
                // compact deliberately does not: the summary would then describe only part of the
                // older half, while installation still replaces *all* of it, deleting the trimmed
                // items with nothing standing in for them. Refusing is the only answer that cannot
                // lose content silently.
                error!(
                    "smart compact: the summarization request exceeded the context window; refusing to compact"
                );
                // Deliberately **not** `set_total_tokens_full`, unlike `compact.rs`'s terminal
                // branch. That would assert the whole thread is over the window, and the inference
                // does not hold here: this request carried the older half *plus* base instructions
                // *plus* the configurable `compact_prompt`, so a large prompt can sink it while a
                // normal request carrying the full thread still fits. Marking the context full
                // would then force a whole-history auto-compaction on the next turn, destroying the
                // verbatim tail this feature exists to keep - right after telling the user nothing
                // changed. `/compact` can afford the assumption because it sends the whole history;
                // smart compact cannot.
                restore_token_accounting_if(sess, turn_context, usage_applied).await;
                sess.track_turn_codex_error(
                    turn_context.as_ref(),
                    &CodexErr::ContextWindowExceeded,
                );
                return Ok(SmartCompactOutcome::Rejected {
                    message: "Smart compact could not summarize: the summarization request exceeded the context window. Nothing was changed; try starting a new thread.".to_string(),
                });
            }
            Err(e) => {
                if retries < max_retries {
                    retries += 1;
                    let delay = backoff(retries);
                    sess.notify_stream_error(
                        turn_context.as_ref(),
                        format!("Reconnecting... {retries}/{max_retries}"),
                        e,
                    )
                    .await;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                restore_token_accounting_if(sess, turn_context, usage_applied).await;
                sess.track_turn_codex_error(turn_context.as_ref(), &e);
                let event = EventMsg::Error(e.to_error_event(/*message_prefix*/ None));
                sess.send_event(turn_context, event).await;
                return Err(e);
            }
        }
    };

    // `summarizer_output` belongs to the successful attempt only and never entered session history,
    // so a failed attempt's prose can neither become the summary nor contaminate the thread.
    let summary_suffix = get_last_assistant_message_from_turn(&summarizer_output);
    let Some(summary_suffix) = summary_suffix.filter(|text| !text.trim().is_empty()) else {
        // Installing an empty summary would delete the older half and replace it with nothing.
        error!("smart compact: summarization produced no assistant message; refusing to compact");
        restore_token_accounting_if(sess, turn_context, usage_applied).await;
        return Ok(SmartCompactOutcome::Rejected {
            message: "Smart compact did not receive a summary from the model, so nothing was compacted. Try again.".to_string(),
        });
    };
    let summary_text = build_summary_item_text(&summary_suffix);

    let split_index = split.split_index;
    if split_index >= pre_turn_items.len() {
        // Unreachable: `select_compaction_split` guarantees `split_index < items.len()`. Clamping
        // instead would silently degrade smart compact into whole-history compaction, discarding
        // the verbatim tail that is the entire point of the feature.
        error!(
            "smart compact: verbatim tail collapsed (split {split_index} >= history {}); refusing to compact",
            pre_turn_items.len()
        );
        restore_token_accounting_if(sess, turn_context, usage_applied).await;
        return Ok(SmartCompactOutcome::Rejected {
            message: "Smart compact could not preserve a verbatim tail. Nothing was changed."
                .to_string(),
        });
    }
    let SmartCompactedHistory {
        items,
        summary_index,
        verbatim_start,
    } = build_smart_compacted_history(
        pre_turn_items,
        split_index,
        &summary_text,
        &turn_context.sub_id,
    );

    // The enforceable half of the per-item cap. `cap_compacted_half_items` shrinks text, but text is
    // not the only thing an item costs, so this confirms the result rather than assuming it. Nothing
    // over the cap is ever installed.
    if let Some((index, estimate)) = oversized_compacted_item(&items[..verbatim_start]) {
        error!(
            "smart compact: compacted item {index} is ~{estimate} tokens, over the {SUMMARY_MAX_TOKENS}-token per-item cap; refusing to compact"
        );
        restore_token_accounting_if(sess, turn_context, usage_applied).await;
        return Ok(SmartCompactOutcome::Rejected {
            message: format!(
                "Smart compact could not fit a compacted message under the {SUMMARY_MAX_TOKENS}-token per-item limit (~{estimate} tokens). Nothing was changed."
            ),
        });
    }
    // The metadata message must be the text that was actually installed, not the pre-cap text, so
    // `CompactedItem.message` and the replacement history can never disagree.
    let summary_text = items
        .get(summary_index)
        .and_then(message_text)
        .unwrap_or(summary_text);

    // Smart compaction must actually reduce context. Unlike `/compact`, which replaces the whole
    // history and therefore always shrinks it, here only the older half is replaced, so a summary
    // larger than the half it replaces makes the thread *bigger*.
    //
    // The older-half estimate deliberately excludes items the next turn will reinject anyway
    // (developer messages and contextual user messages, cleared by `DoNotInject` and rebuilt in
    // full). Counting them would credit smart compact for a reduction that lasts exactly one
    // request. This is a coarse guard on a coarse byte-based estimate, not accounting: it exists to
    // catch a summary that is grossly larger than what it replaces.
    let older_estimate = estimate_replaceable_token_count(&pre_turn_items[..split_index]);
    let compacted_estimate = estimate_items_token_count(&items[..verbatim_start]);
    if compacted_estimate >= older_estimate {
        error!(
            "smart compact: compacted half is not smaller (~{compacted_estimate} vs ~{older_estimate} tokens); refusing to compact"
        );
        restore_token_accounting_if(sess, turn_context, usage_applied).await;
        return Ok(SmartCompactOutcome::Rejected {
            message: format!(
                "Smart compact would not reduce context: the summary plus retained messages (~{compacted_estimate} tokens) is at least as large as the {split_index} items it would replace (~{older_estimate} tokens). Nothing was changed."
            ),
        });
    }

    // Manual/standalone compaction variants use `DoNotInject`: no initial context is written into
    // the replacement history, and clearing both baselines makes the next regular turn reinject
    // context state in full. Routed through the shared helper so the decision stays in lockstep
    // with `compact.rs` if that contract ever changes.
    let (initial_context, world_state_baseline) =
        build_compaction_initial_context(sess.as_ref(), &InitialContextInjection::DoNotInject)
            .await;
    debug_assert!(
        initial_context.is_empty(),
        "DoNotInject must not build initial context"
    );
    debug_assert!(
        world_state_baseline.is_none(),
        "DoNotInject must not set a world-state baseline"
    );

    let older_items = split_index;
    let newer_items = pre_turn_items.len().saturating_sub(split_index);
    // Everything that could refuse has now passed, so the compaction really is happening: start the
    // turn item here and complete it below.
    let compaction_item = TurnItem::ContextCompaction(ContextCompactionItem::new());
    sess.emit_turn_item_started(turn_context, &compaction_item)
        .await;
    let (window_number, window_ids) = sess.advance_auto_compact_window().await;
    sess.replace_compacted_history(
        items,
        /*reference_context_item*/ None,
        world_state_baseline,
        CompactedHistoryMetadata {
            message: summary_text,
            window_number,
            window_ids,
        },
    )
    .await;
    sess.recompute_token_usage(turn_context).await;
    sess.emit_turn_item_completed(turn_context, compaction_item)
        .await;

    Ok(SmartCompactOutcome::Compacted {
        older_items,
        newer_items,
        older_token_estimate: split.older_token_estimate,
        newer_token_estimate: split.newer_token_estimate,
        preexisting_violations: split.preexisting_violations.len(),
    })
}

#[cfg(test)]
#[path = "compact_smart_tests.rs"]
mod tests;
