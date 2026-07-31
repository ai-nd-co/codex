use super::*;
use crate::compact::is_summary_message;
use crate::context_manager::split::tests::assistant_message;
use crate::context_manager::split::tests::contextual_developer_message;
use crate::context_manager::split::tests::contextual_user_message;
use crate::context_manager::split::tests::developer_message;
use crate::context_manager::split::tests::for_prompt_roundtrip;
use crate::context_manager::split::tests::function_call;
use crate::context_manager::split::tests::function_call_output;
use crate::context_manager::split::tests::reasoning;
use crate::context_manager::split::tests::user_message;
use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;

const TURN_ID: &str = "turn-compaction";
const SUMMARY_BODY: &str = "SUMMARY_BODY";

fn summary_text() -> String {
    build_summary_item_text(SUMMARY_BODY)
}

/// Three complete turns, each with a closed tool pair and reasoning, so every candidate boundary is
/// closed and the selector has a real choice to make.
fn three_turn_history() -> Vec<ResponseItem> {
    vec![
        user_message("u1"),
        reasoning("r1"),
        function_call("c1"),
        function_call_output("c1"),
        assistant_message("a1"),
        user_message("u2"),
        reasoning("r2"),
        function_call("c2"),
        function_call_output("c2"),
        assistant_message("a2"),
        user_message("u3"),
        reasoning("r3"),
        assistant_message("a3"),
    ]
}

fn message_texts(items: &[ResponseItem]) -> Vec<String> {
    items.iter().filter_map(message_text).collect()
}

/// The selector must accept this fixture; a rejection here means the fixture, not the feature,
/// is wrong.
fn split_of(items: &[ResponseItem]) -> usize {
    select_compaction_split(items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect("fixture history should have a safe split")
        .split_index
}

#[test]
fn smart_compact_is_disabled_by_default() {
    let spec = codex_features::FEATURES
        .iter()
        .find(|spec| spec.id == Feature::SmartCompact)
        .expect("smart_compact feature spec should be registered");
    assert_eq!(spec.key, "smart_compact");
    assert!(
        !spec.default_enabled,
        "smart compact must ship default-off: it deliberately places the summary before a verbatim tail"
    );
}

/// `AGENTS.md` "Model visible context" rules 3 and 4: bounded size, never above 10K tokens. Nothing
/// downstream enforces this - `replace_compacted_history` installs through `ContextManager::replace`,
/// which bypasses the `record_items` truncation path.
#[test]
fn the_summary_item_is_hard_capped_at_ten_thousand_tokens() {
    let short = build_summary_item_text("a short summary");
    assert!(short.ends_with("a short summary"), "got {short:?}");
    assert!(is_summary_message(&short));

    let oversized = build_summary_item_text(&"token ".repeat(200_000));
    let tokens = approx_token_count(&oversized);
    assert!(
        tokens <= SUMMARY_MAX_TOKENS,
        "summary item must be capped at {SUMMARY_MAX_TOKENS} tokens, got ~{tokens}"
    );
    assert!(
        approx_token_count(&"token ".repeat(200_000)) > SUMMARY_MAX_TOKENS,
        "the fixture must actually exceed the cap or this test proves nothing"
    );
    assert!(
        is_summary_message(&oversized),
        "truncation must not eat the prefix the rest of the system matches on"
    );
}

/// The non-shrinking guard must not credit smart compact for removing context that
/// `InitialContextInjection::DoNotInject` causes the very next turn to reinject...
#[test]
fn reinjected_context_is_not_counted_as_removable() {
    let items = vec![
        contextual_developer_message(),
        contextual_user_message(),
        user_message("u1"),
        assistant_message("a1"),
    ];
    let all = estimate_items_token_count(&items);
    let replaceable = estimate_replaceable_token_count(&items);
    assert!(
        replaceable < all,
        "reinjected context must be excluded from the removable estimate ({replaceable} vs {all})"
    );
    assert_eq!(
        replaceable,
        estimate_items_token_count(&items[2..]),
        "exactly the two contextual items should be excluded"
    );
}

/// ...but it must not exclude *every* developer message. Historical developer items (guardian
/// approvals, interrupt markers, the multi-agent hint) are removed permanently, so excluding them
/// would make the guard refuse compactions that really do shrink the persistent context.
#[test]
fn non_contextual_developer_history_still_counts_as_removable() {
    let items = vec![
        developer_message("a guardian approval record from an earlier turn"),
        user_message("u1"),
    ];
    assert_eq!(
        estimate_replaceable_token_count(&items),
        estimate_items_token_count(&items),
        "non-contextual developer history is not reinjected, so it counts as removable"
    );
}

/// The per-item cap applies to retained user messages too, not just the summary.
/// `build_compacted_history` bounds them only in aggregate.
#[test]
fn retained_user_messages_are_capped_per_item() {
    let huge = "word ".repeat(200_000);
    let items = vec![
        user_message(&huge),
        assistant_message("a1"),
        user_message("u2"),
        assistant_message("a2"),
    ];
    let split_index = split_of(&items);
    assert!(
        split_index >= 1,
        "the huge user message must land in the older half"
    );
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    // The bound is on the whole item, the way `estimate_item_token_count` measures every other item
    // in the context, not on its text alone.
    for item in &built.items[..built.verbatim_start] {
        let estimate = estimate_item_token_count(item);
        assert!(
            estimate <= SUMMARY_MAX_TOKENS as i64,
            "compacted-half item exceeds the per-item cap: ~{estimate} tokens"
        );
    }
    // The verbatim tail is pre-existing history and must not be rewritten by the cap.
    assert_eq!(&built.items[built.verbatim_start..], &items[split_index..]);

    // The summary is already capped upstream, so the per-item pass must leave it byte-identical:
    // re-truncating would make the installed text diverge from `CompactedItem.message`.
    assert_eq!(
        message_text(&built.items[built.summary_index]),
        Some(build_summary_item_text(SUMMARY_BODY))
    );
    // The cap is enforceable, not best-effort.
    assert_eq!(
        oversized_compacted_item(&built.items[..built.verbatim_start]),
        None
    );
    // ...and it was measured against what actually gets installed: ids are assigned here, so
    // `Session::replace_compacted_history` cannot add bytes after the check.
    for item in &built.items[..built.verbatim_start] {
        assert!(
            item.id().is_some_and(|id| !id.is_empty()),
            "compacted-half items must already carry their ids when the cap is measured"
        );
    }
    // The summary keeps its prefix; nothing in the cap path may erase it.
    assert!(is_summary_message(
        &message_text(&built.items[built.summary_index]).unwrap_or_default()
    ));
}

/// A summary that is under the cap as raw text but over it once JSON-escaped gets truncated a
/// second time by the whole-item pass. That is exactly the case where the install path must read the
/// metadata message back out of the built item instead of reusing its own pre-cap string, so this
/// test establishes that the two values really can differ.
#[test]
fn escape_heavy_summaries_are_re_truncated_by_the_whole_item_cap() {
    // Every character escapes to two bytes in JSON, so the serialized item is about twice the raw
    // text while `build_summary_item_text` only measures the raw text.
    let escape_heavy = build_summary_item_text(&"\"\\".repeat(20_000));
    assert!(
        approx_token_count(&escape_heavy) <= SUMMARY_MAX_TOKENS,
        "the pre-cap text must already satisfy the raw-text budget"
    );

    let items = vec![
        user_message("u1"),
        assistant_message("a1"),
        user_message("u2"),
        assistant_message("a2"),
    ];
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &escape_heavy, TURN_ID);
    let installed = message_text(&built.items[built.summary_index]).unwrap_or_default();

    assert_ne!(
        installed, escape_heavy,
        "an escape-heavy summary must be shortened again by the whole-item cap; if this stops \
         holding, the metadata-readback in summarize_and_install is no longer load-bearing"
    );
    assert_eq!(
        oversized_compacted_item(&built.items[..built.verbatim_start]),
        None
    );
    assert!(
        is_summary_message(&installed),
        "the second truncation must not eat the prefix"
    );
}

/// `oversized_compacted_item` is the postcondition the install path refuses on. A message whose
/// non-text cost alone blows the cap cannot be rescued by truncating text, and must be reported
/// rather than installed or silently emptied.
#[test]
fn an_item_that_truncation_cannot_rescue_is_reported_not_installed() {
    let mut item = user_message("x");
    // An absurd id stands in for any non-text cost; the estimator serializes the whole item.
    item.set_id(Some(ResponseItemId::from_server("y".repeat(80_000))));
    assert!(
        estimate_item_token_count(&item) > SUMMARY_MAX_TOKENS as i64,
        "fixture must exceed the cap through non-text cost alone"
    );

    let mut items = vec![item];
    cap_compacted_half_items(&mut items);
    let reported = oversized_compacted_item(&items);
    assert!(
        reported.is_some(),
        "an unrescuable item must be reported, not silently accepted"
    );
    assert_eq!(reported.map(|(index, _)| index), Some(0));
    assert_eq!(
        message_text(&items[0]).as_deref(),
        Some("x"),
        "text must not be emptied when truncation cannot help"
    );
}

#[test]
fn newer_half_is_byte_identical_in_the_replacement_history() {
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    assert_eq!(
        &built.items[built.verbatim_start..],
        &items[split_index..],
        "the newer half must survive byte-identically, not merely semantically"
    );
    assert_eq!(
        built.items.len() - built.verbatim_start,
        items.len() - split_index
    );
}

#[test]
fn summary_sits_immediately_before_the_verbatim_tail() {
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    assert_eq!(built.verbatim_start, built.summary_index + 1);
    let summary = message_text(&built.items[built.summary_index]).expect("summary is a message");
    assert!(
        is_summary_message(&summary),
        "expected a compaction summary at summary_index, got {summary:?}"
    );
    assert!(
        built.items[..built.summary_index]
            .iter()
            .filter_map(message_text)
            .all(|text| !is_summary_message(&text)),
        "there must be exactly one summary and it must be the last item of the compacted half"
    );
}

#[test]
fn older_half_user_messages_are_retained_and_the_older_tool_layer_is_dropped() {
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    let older_user_texts: Vec<String> = message_texts(&items[..split_index])
        .into_iter()
        .filter(|text| text.starts_with('u'))
        .collect();
    assert!(
        !older_user_texts.is_empty(),
        "fixture should place at least one user message in the older half"
    );
    let compacted_texts = message_texts(&built.items[..built.verbatim_start]);
    for text in &older_user_texts {
        assert!(
            compacted_texts.contains(text),
            "older-half user message {text:?} should survive retention"
        );
    }

    // The whole tool and reasoning layer of the older half is replaced by the summary, exactly as
    // `compact_remote::should_keep_compacted_history_item` does for whole-history compaction.
    assert!(
        built.items[..built.verbatim_start]
            .iter()
            .all(|item| matches!(item, ResponseItem::Message { .. })),
        "the compacted half must contain messages only"
    );
    assert!(
        !built.items[..built.verbatim_start]
            .iter()
            .any(|item| matches!(item, ResponseItem::FunctionCall { .. })),
        "older-half function calls must not survive compaction"
    );
}

#[test]
fn newer_half_user_messages_are_not_duplicated_into_the_compacted_half() {
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    let newer_user_texts: Vec<String> = message_texts(&items[split_index..])
        .into_iter()
        .filter(|text| text.starts_with('u'))
        .collect();
    let compacted_texts = message_texts(&built.items[..built.verbatim_start]);
    for text in &newer_user_texts {
        assert!(
            !compacted_texts.contains(text),
            "newer-half user message {text:?} is already verbatim; retaining it again would duplicate it"
        );
    }
}

#[test]
fn a_previous_compaction_summary_in_the_older_half_is_not_re_retained() {
    let mut items = three_turn_history();
    let stale_summary = format!("{SUMMARY_PREFIX}\nSTALE_SUMMARY");
    items.insert(0, user_message(&stale_summary));
    let split_index = split_of(&items);
    assert!(
        split_index > 0,
        "the stale summary must land in the older half for this test to mean anything"
    );

    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);
    let compacted_summaries: Vec<String> = message_texts(&built.items[..built.verbatim_start])
        .into_iter()
        .filter(|text| is_summary_message(text))
        .collect();
    assert_eq!(
        compacted_summaries,
        vec![summary_text()],
        "only the new summary should remain; the stale one is covered by it"
    );
}

#[test]
fn only_the_summary_is_stamped_with_the_compaction_turn_id() {
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    assert_eq!(built.items[built.summary_index].turn_id(), Some(TURN_ID));

    for item in &built.items[built.verbatim_start..] {
        assert_ne!(
            item.turn_id(),
            Some(TURN_ID),
            "verbatim items keep their own turn id and must not be restamped"
        );
    }
}

/// The real proof, reusing S1's harness: push the installed history through the production
/// normalization path in a debug build, where `crate::util::error_or_panic` panics.
///
/// Equality (not merely "did not panic") is the stronger assertion: a fabricated `"aborted"` output
/// or a dropped orphan would change the item list even on the silent code paths.
#[test]
#[allow(clippy::assertions_on_constants)]
fn installed_history_survives_for_prompt_in_a_debug_build() {
    // Deliberately a compile-time constant, matching S1: it fails loudly if someone runs this in a
    // release profile and assumes the proof still holds.
    assert!(
        cfg!(debug_assertions),
        "this proof is only meaningful when error_or_panic panics"
    );
    let items = three_turn_history();
    let split_index = split_of(&items);
    let built = build_smart_compacted_history(&items, split_index, &summary_text(), TURN_ID);

    assert_eq!(for_prompt_roundtrip(&built.items), built.items);
    // The summarizer request is normalized too, so the older half must be closed on its own.
    assert_eq!(
        for_prompt_roundtrip(&items[..split_index]),
        items[..split_index].to_vec()
    );
}

/// A dangling `FunctionCall` left by an interrupted turn is a normal steady state
/// (`stream_events_utils` persists the call immediately; the output only lands when the tool
/// resolves). Smart compact must still work, and must not turn that into an induced violation.
#[test]
fn a_preexisting_dangling_call_does_not_block_smart_compaction() {
    let items = vec![
        user_message("u1"),
        function_call("orphaned"),
        user_message("u2"),
        reasoning("r2"),
        function_call("c2"),
        function_call_output("c2"),
        assistant_message("a2"),
        user_message("u3"),
        assistant_message("a3"),
    ];
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect("a pre-existing dangling call must not make the history uncompactable");
    assert!(
        !split.preexisting_violations.is_empty(),
        "fixture should actually contain a pre-existing violation"
    );

    let built = build_smart_compacted_history(&items, split.split_index, &summary_text(), TURN_ID);
    assert_eq!(
        &built.items[built.verbatim_start..],
        &items[split.split_index..]
    );
}

#[test]
fn every_split_rejection_variant_has_a_distinct_actionable_message() {
    let rejections = [
        SplitRejection::EmptyHistory,
        SplitRejection::NoTurnBoundary,
        SplitRejection::NoInteriorTurnBoundary { boundaries: 1 },
        SplitRejection::NoSafeBoundary {
            candidates_tried: 2,
            unusable_candidates: 1,
            nearest_induced: Vec::new(),
        },
    ];
    let mut messages = Vec::new();
    for rejection in &rejections {
        let message = describe_split_rejection(rejection);
        assert!(
            message.len() > 20 && message.ends_with('.'),
            "rejection {rejection:?} produced a poor message: {message:?}"
        );
        messages.push(message);
    }
    messages.sort();
    let unique = messages.len();
    messages.dedup();
    assert_eq!(
        messages.len(),
        unique,
        "each rejection reason must be distinguishable by its message"
    );
}

/// `select_compaction_split` rejects rather than returning an unsafe index, and each rejection is
/// reported to the user instead of silently doing nothing.
#[test]
fn histories_without_an_interior_turn_boundary_are_rejected_not_silently_compacted() {
    assert_eq!(
        select_compaction_split(&[], DEFAULT_SPLIT_TARGET_FRACTION),
        Err(SplitRejection::EmptyHistory)
    );
    assert_eq!(
        select_compaction_split(&[assistant_message("a1")], DEFAULT_SPLIT_TARGET_FRACTION),
        Err(SplitRejection::NoTurnBoundary)
    );
    assert_eq!(
        select_compaction_split(
            &[user_message("u1"), assistant_message("a1")],
            DEFAULT_SPLIT_TARGET_FRACTION
        ),
        Err(SplitRejection::NoInteriorTurnBoundary { boundaries: 1 })
    );
}
