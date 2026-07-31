use super::*;
use crate::context_manager::ContextManager;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::LocalShellAction;
use codex_protocol::models::LocalShellExecAction;
use codex_protocol::models::LocalShellStatus;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::InputModality;
use codex_protocol::protocol::ENVIRONMENTS_INSTRUCTIONS_OPEN_TAG;
use pretty_assertions::assert_eq;

/// Every modality enabled, so `for_prompt` performs no image/audio stripping and the only remaining
/// transformations are the two pairing repairs this module exists to prevent.
pub(crate) const ALL_MODALITIES: &[InputModality] = &[
    InputModality::Text,
    InputModality::Image,
    InputModality::Audio,
];

/// Matched by `LegacyUnifiedExecProcessLimitWarning::matches_text`, which makes a user message
/// contextual and therefore not a turn boundary.
const CONTEXTUAL_USER_TEXT: &str =
    "Warning: The maximum number of unified exec processes you can keep open is 3.";

// ---------------------------------------------------------------------------
// item builders
// ---------------------------------------------------------------------------

pub(crate) fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn assistant_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn developer_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

/// Developer message whose only fragment is a rollback-trimmable contextual one.
pub(crate) fn contextual_developer_message() -> ResponseItem {
    developer_message(&format!("{ENVIRONMENTS_INSTRUCTIONS_OPEN_TAG}\nlinux\n"))
}

/// `build_initial_context`-style bundle: contextual fragment plus persistent developer text.
fn mixed_developer_bundle() -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![
            ContentItem::InputText {
                text: format!("{ENVIRONMENTS_INSTRUCTIONS_OPEN_TAG}\nlinux\n"),
            },
            ContentItem::InputText {
                text: "Persistent developer instructions.".to_string(),
            },
        ],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn contextual_user_message() -> ResponseItem {
    user_message(CONTEXTUAL_USER_TEXT)
}

fn agent_message(text: &str) -> ResponseItem {
    ResponseItem::AgentMessage {
        id: None,
        author: "planner".to_string(),
        recipient: "worker".to_string(),
        content: vec![AgentMessageInputContent::InputText {
            text: text.to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn reasoning(text: &str) -> ResponseItem {
    ResponseItem::Reasoning {
        id: None,
        summary: Vec::new(),
        content: None,
        encrypted_content: Some(text.to_string()),
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn function_call(call_id: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        id: None,
        name: "shell".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        call_id: call_id.to_string(),
        internal_chat_message_metadata_passthrough: None,
    }
}

pub(crate) fn function_call_output(call_id: &str) -> ResponseItem {
    ResponseItem::FunctionCallOutput {
        id: None,
        call_id: call_id.to_string(),
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn local_shell_call(call_id: &str) -> ResponseItem {
    ResponseItem::LocalShellCall {
        id: None,
        call_id: Some(call_id.to_string()),
        status: LocalShellStatus::Completed,
        action: LocalShellAction::Exec(LocalShellExecAction {
            command: vec!["ls".to_string()],
            timeout_ms: None,
            working_directory: None,
            env: None,
            user: None,
        }),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn custom_tool_call(call_id: &str) -> ResponseItem {
    ResponseItem::CustomToolCall {
        id: None,
        status: None,
        call_id: call_id.to_string(),
        name: "custom".to_string(),
        namespace: None,
        input: "in".to_string(),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn custom_tool_call_output(call_id: &str) -> ResponseItem {
    ResponseItem::CustomToolCallOutput {
        id: None,
        call_id: call_id.to_string(),
        name: None,
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn tool_search_call(call_id: &str) -> ResponseItem {
    ResponseItem::ToolSearchCall {
        id: None,
        call_id: Some(call_id.to_string()),
        status: None,
        execution: "client".to_string(),
        arguments: serde_json::Value::Null,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn tool_search_output(call_id: Option<&str>, execution: &str) -> ResponseItem {
    ResponseItem::ToolSearchOutput {
        id: None,
        call_id: call_id.map(str::to_string),
        status: "completed".to_string(),
        execution: execution.to_string(),
        tools: Vec::new(),
        internal_chat_message_metadata_passthrough: None,
    }
}

fn web_search_call() -> ResponseItem {
    ResponseItem::WebSearchCall {
        id: None,
        status: Some("completed".to_string()),
        action: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn additional_tools() -> ResponseItem {
    ResponseItem::AdditionalTools {
        id: None,
        role: "developer".to_string(),
        tools: Vec::new(),
    }
}

/// Stamp an item the way `Session::prepare_conversation_items_for_history` does
/// (`session/mod.rs:2813-2816`). Metadata-less variants silently ignore this, as they do in
/// production.
fn stamped(mut item: ResponseItem, turn_id: &str) -> ResponseItem {
    item.set_turn_id_if_missing(turn_id);
    item
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Run a half through the real prompt-normalization path.
///
/// `for_prompt` is what a violation would trip: `error_or_panic` panics under `debug_assertions`,
/// so in a debug build this call *is* the proof that the split is safe.
pub(crate) fn for_prompt_roundtrip(items: &[ResponseItem]) -> Vec<ResponseItem> {
    let mut manager = ContextManager::new();
    manager.replace(items.to_vec());
    manager.for_prompt(ALL_MODALITIES)
}

/// The target fraction that makes `candidate` the token-weighted target, so a test can aim the
/// selector at one specific boundary without hard-coding token estimates.
fn fraction_targeting(items: &[ResponseItem], candidate: usize) -> f64 {
    let prefix = items
        .iter()
        .take(candidate)
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add);
    let total = items
        .iter()
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add);
    assert!(total > 0, "token estimate for the fixture must be positive");
    prefix as f64 / total as f64
}

fn induced_kinds(report: &ClosureReport) -> Vec<ClosureViolationKind> {
    report
        .induced
        .iter()
        .map(|violation| violation.kind)
        .collect()
}

/// Re-derived from the item shape and the `event_mapping` primitive rather than from
/// `snap_over_pre_turn_context`, so the production code is not its own oracle.
fn is_pre_turn_context_item(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { role, .. } if role == "developer" => true,
        ResponseItem::Message { role, content, .. } if role == "user" => {
            crate::event_mapping::is_contextual_user_message_content(content)
        }
        _ => false,
    }
}

/// Independent structural check that `split_index` is derived from a turn boundary: either it *is*
/// one, or every item from it up to the next boundary is a pre-turn context item belonging to that
/// boundary's turn, and the run was not cut in half.
fn assert_derived_from_turn_boundary(items: &[ResponseItem], split_index: usize) {
    let boundary = items
        .iter()
        .enumerate()
        .skip(split_index)
        .find(|(_, item)| is_user_turn_boundary(item))
        .map(|(index, _)| index);
    let Some(boundary) = boundary else {
        panic!("split index {split_index} is not followed by any turn boundary");
    };
    for (index, item) in items.iter().enumerate().take(boundary).skip(split_index) {
        assert!(
            is_pre_turn_context_item(item),
            "item {index} between split {split_index} and boundary {boundary} is not a pre-turn \
             context item: {item:?}"
        );
    }
    if let Some(previous) = split_index.checked_sub(1) {
        assert!(
            !is_pre_turn_context_item(&items[previous]),
            "split {split_index} cut a pre-turn context run: item {previous} still belongs to the \
             turn at {boundary}"
        );
    }
}

/// Assert no reasoning item was separated from the turn that produced it.
fn assert_reasoning_travels_with_its_turn(items: &[ResponseItem], split_index: usize) {
    for (index, item) in items.iter().enumerate() {
        if !matches!(item, ResponseItem::Reasoning { .. }) {
            continue;
        }
        let owner = items
            .iter()
            .take(index + 1)
            .enumerate()
            .filter(|(_, candidate)| is_user_turn_boundary(candidate))
            .map(|(owner_index, _)| owner_index)
            .next_back();
        let Some(owner) = owner else {
            continue;
        };
        assert_eq!(
            owner < split_index,
            index < split_index,
            "reasoning at {index} was separated from its turn boundary at {owner} by split \
             {split_index}"
        );
    }
}

// ---------------------------------------------------------------------------
// the debug-build precondition
// ---------------------------------------------------------------------------

/// The `for_prompt` assertions in this file are only a *proof* when `error_or_panic` panics, which
/// it does under `debug_assertions`. Asserting a compile-time constant is deliberate: it fails the
/// suite loudly if someone runs it in a release profile and assumes the proof still holds.
#[test]
#[allow(clippy::assertions_on_constants)]
fn debug_assertions_are_enabled_so_normalization_panics_on_violation() {
    assert!(
        cfg!(debug_assertions),
        "the closure proof relies on error_or_panic panicking; run these tests in a debug build"
    );
}

// ---------------------------------------------------------------------------
// closure validation
// ---------------------------------------------------------------------------

#[test]
fn validate_split_closure_reports_dangling_call_and_orphan_output_sides() {
    let items = vec![
        user_message("hi"),
        function_call("c1"),
        function_call_output("c1"),
    ];
    let report = validate_split_closure(&items, 2);

    assert_eq!(
        induced_kinds(&report),
        vec![
            ClosureViolationKind::DanglingFunctionCall,
            ClosureViolationKind::OrphanFunctionCallOutput,
        ]
    );
    assert_eq!(report.induced[0].index, 1);
    assert_eq!(report.induced[0].side, SplitSide::Older);
    assert_eq!(report.induced[0].call_id, "c1");
    assert_eq!(report.induced[1].index, 2);
    assert_eq!(report.induced[1].side, SplitSide::Newer);
    assert!(report.preexisting.is_empty());
    assert!(!report.is_closed());
}

#[test]
fn validate_split_closure_accepts_a_boundary_that_keeps_pairs_together() {
    let items = vec![
        user_message("hi"),
        function_call("c1"),
        function_call_output("c1"),
        user_message("again"),
        function_call("c2"),
        function_call_output("c2"),
    ];
    let report = validate_split_closure(&items, 3);

    assert!(report.is_closed());
    assert!(report.preexisting.is_empty());
}

#[test]
fn validate_split_closure_pairs_local_shell_call_with_function_call_output() {
    let items = vec![
        user_message("hi"),
        local_shell_call("c1"),
        function_call_output("c1"),
    ];

    assert!(validate_split_closure(&items, 3).is_closed());
    assert_eq!(
        induced_kinds(&validate_split_closure(&items, 2)),
        vec![
            ClosureViolationKind::DanglingLocalShellCall,
            ClosureViolationKind::OrphanFunctionCallOutput,
        ]
    );
}

/// `FunctionCall` and `LocalShellCall` share the `FunctionCallOutput` id namespace, so a single
/// output can satisfy both in the whole history yet only one of them after a split.
#[test]
fn validate_split_closure_detects_shared_output_namespace_collision() {
    let items = vec![
        user_message("hi"),
        function_call("shared"),
        function_call_output("shared"),
        user_message("again"),
        local_shell_call("shared"),
    ];

    // Whole history: the output satisfies both call variants, so nothing is unpaired.
    assert!(validate_split_closure(&items, 5).is_closed());
    // Split at the second turn: the local shell call loses the only output.
    let report = validate_split_closure(&items, 3);
    assert_eq!(
        induced_kinds(&report),
        vec![ClosureViolationKind::DanglingLocalShellCall]
    );
    assert_eq!(report.induced[0].side, SplitSide::Newer);
}

/// Asserts against a fully empty report, not just `is_closed()`: `is_closed()` ignores `preexisting`,
/// so it alone would not notice the exemption being misclassified as a pre-existing orphan.
#[test]
fn validate_split_closure_ignores_server_tool_search_output() {
    let items = vec![
        user_message("hi"),
        tool_search_output(Some("never-called"), "server"),
        assistant_message("done"),
    ];

    for split_index in 0..=items.len() {
        assert_eq!(
            validate_split_closure(&items, split_index),
            ClosureReport::default(),
            "server-executed tool search output must never be reported (split {split_index})"
        );
    }
}

/// From codex audit round 5: `ensure_call_outputs_present` populates `tool_search_output_ids` from
/// *every* `ToolSearchOutput` with a `call_id`, including server-executed ones (`normalize.rs:29-34`),
/// so a server output does satisfy its call. Only the orphan direction exempts them.
#[test]
fn validate_split_closure_lets_a_server_tool_search_output_satisfy_its_call() {
    let items = vec![
        user_message("turn 0"),
        tool_search_call("x"),
        user_message("turn 1"),
        tool_search_output(Some("x"), "server"),
    ];

    // Whole history: the server output answers the call, so nothing is reported at all.
    assert_eq!(
        validate_split_closure(&items, items.len()),
        ClosureReport::default()
    );
    // Split between them: the call is left dangling, and that is induced, not pre-existing.
    let report = validate_split_closure(&items, 2);
    assert_eq!(
        induced_kinds(&report),
        vec![ClosureViolationKind::DanglingToolSearchCall]
    );
    assert_eq!(report.induced[0].side, SplitSide::Older);
    assert!(report.preexisting.is_empty());
    assert!(!report.is_closed());
}

#[test]
fn validate_split_closure_ignores_calls_and_outputs_without_a_call_id() {
    let items = vec![
        user_message("hi"),
        tool_search_output(None, "client"),
        ResponseItem::LocalShellCall {
            id: None,
            call_id: None,
            status: LocalShellStatus::Completed,
            action: LocalShellAction::Exec(LocalShellExecAction {
                command: vec!["ls".to_string()],
                timeout_ms: None,
                working_directory: None,
                env: None,
                user: None,
            }),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::ToolSearchCall {
            id: None,
            call_id: None,
            status: None,
            execution: "client".to_string(),
            arguments: serde_json::Value::Null,
            internal_chat_message_metadata_passthrough: None,
        },
        assistant_message("done"),
    ];

    for split_index in 0..=items.len() {
        assert_eq!(
            validate_split_closure(&items, split_index),
            ClosureReport::default(),
            "items without a call id require no counterpart (split {split_index})"
        );
    }
}

/// From codex audit round 3: `call_id: None` items are exempt from pairing, so a split between a
/// `ToolSearchCall { call_id: None }` and a `ToolSearchOutput { call_id: None }` must be *retained*, not
/// snapped away. A regression that treated absent ids as one shared empty id would falsely reject it.
#[test]
fn select_compaction_split_retains_a_boundary_between_call_id_less_items() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(
            ResponseItem::ToolSearchCall {
                id: None,
                call_id: None,
                status: None,
                execution: "client".to_string(),
                arguments: serde_json::Value::Null,
                internal_chat_message_metadata_passthrough: None,
            },
            "turn-0",
        ),
        stamped(user_message("turn 1"), "turn-1"),
        stamped(tool_search_output(None, "client"), "turn-1"),
        stamped(assistant_message("done 1"), "turn-1"),
    ];

    assert_eq!(split_candidates(&items), vec![3]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 3))
        .expect("candidate 3 is safe: neither item needs a counterpart");
    assert_eq!(split.split_index, 3);
    assert_eq!(split.snapped_outward_by, 0);
    assert!(split.preexisting_violations.is_empty());
}

#[test]
fn validate_split_closure_reports_custom_and_tool_search_pairs() {
    let items = vec![
        user_message("hi"),
        custom_tool_call("c1"),
        tool_search_call("c2"),
        custom_tool_call_output("c1"),
        tool_search_output(Some("c2"), "client"),
    ];

    assert!(validate_split_closure(&items, 5).is_closed());
    let report = validate_split_closure(&items, 3);
    assert_eq!(
        induced_kinds(&report),
        vec![
            ClosureViolationKind::DanglingCustomToolCall,
            ClosureViolationKind::DanglingToolSearchCall,
            ClosureViolationKind::OrphanCustomToolCallOutput,
            ClosureViolationKind::OrphanToolSearchOutput,
        ]
    );
}

/// Out-of-order closure: two calls are issued before either output arrives. Adjacency-based pairing
/// would accept the cut between the outputs; set-based pairing does not.
#[test]
fn validate_split_closure_handles_out_of_order_pairs() {
    let items = vec![
        user_message("hi"),
        function_call("a"),
        function_call("b"),
        function_call_output("a"),
        function_call_output("b"),
    ];

    assert!(validate_split_closure(&items, 5).is_closed());
    let report = validate_split_closure(&items, 4);
    assert_eq!(
        induced_kinds(&report),
        vec![
            ClosureViolationKind::DanglingFunctionCall,
            ClosureViolationKind::OrphanFunctionCallOutput,
        ]
    );
    assert_eq!(report.induced[0].call_id, "b");
    assert_eq!(report.induced[1].call_id, "b");
}

#[test]
fn validate_split_closure_classifies_preexisting_violations_separately() {
    // `c1` was interrupted before its output was recorded, exactly as
    // `stream_events_utils::handle_output_item_done` plus an interrupt leaves history.
    let items = vec![
        user_message("hi"),
        function_call("c1"),
        user_message("again"),
        function_call("c2"),
        function_call_output("c2"),
    ];
    let report = validate_split_closure(&items, 2);

    assert!(report.is_closed(), "the split induced nothing");
    assert_eq!(report.preexisting.len(), 1);
    assert_eq!(
        report.preexisting[0].kind,
        ClosureViolationKind::DanglingFunctionCall
    );
    assert_eq!(report.preexisting[0].index, 1);
    assert_eq!(report.preexisting[0].side, SplitSide::Older);
}

/// From codex audit round 5: only two of the seven kinds had independent pre-existing coverage. This
/// pins all seven, by planting each unpaired item in the *middle* of a two-turn history and asserting
/// that every candidate split reports it as pre-existing and induces nothing.
///
/// `for_prompt` is deliberately not called: five of the seven kinds route through `error_or_panic`,
/// which panics under `debug_assertions`, and that is the behaviour today with or without a split.
#[test]
fn validate_split_closure_classifies_every_kind_as_preexisting_when_it_was_already_unpaired() {
    let cases: Vec<(ClosureViolationKind, ResponseItem)> = vec![
        (
            ClosureViolationKind::DanglingFunctionCall,
            function_call("solo"),
        ),
        (
            ClosureViolationKind::DanglingToolSearchCall,
            tool_search_call("solo"),
        ),
        (
            ClosureViolationKind::DanglingCustomToolCall,
            custom_tool_call("solo"),
        ),
        (
            ClosureViolationKind::DanglingLocalShellCall,
            local_shell_call("solo"),
        ),
        (
            ClosureViolationKind::OrphanFunctionCallOutput,
            function_call_output("solo"),
        ),
        (
            ClosureViolationKind::OrphanCustomToolCallOutput,
            custom_tool_call_output("solo"),
        ),
        (
            ClosureViolationKind::OrphanToolSearchOutput,
            tool_search_output(Some("solo"), "client"),
        ),
    ];

    for (kind, unpaired) in cases {
        let items = vec![
            user_message("turn 0"),
            unpaired.clone(),
            assistant_message("done 0"),
            user_message("turn 1"),
            assistant_message("done 1"),
        ];
        // Present in the whole history, so no split can be blamed for it.
        let whole = validate_split_closure(&items, items.len());
        assert_eq!(whole.induced, Vec::new(), "kind {kind:?}");
        assert_eq!(whole.preexisting.len(), 1, "kind {kind:?}");
        assert_eq!(whole.preexisting[0].kind, kind);
        assert_eq!(whole.preexisting[0].index, 1);

        for split_index in 0..=items.len() {
            let report = validate_split_closure(&items, split_index);
            assert!(
                report.is_closed(),
                "kind {kind:?}: split {split_index} wrongly blamed a pre-existing violation"
            );
            assert_eq!(report.preexisting.len(), 1, "kind {kind:?}");
            assert_eq!(report.preexisting[0].kind, kind, "kind {kind:?}");
            assert_eq!(
                report.preexisting[0].side,
                if split_index > 1 {
                    SplitSide::Older
                } else {
                    SplitSide::Newer
                },
                "kind {kind:?}: wrong side at split {split_index}"
            );
        }

        // And the selector accepts a boundary rather than rejecting every one of them.
        let split =
            select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("safe split");
        assert_eq!(split.split_index, 3, "kind {kind:?}");
        assert_eq!(split.preexisting_violations.len(), 1, "kind {kind:?}");
        assert_eq!(split.preexisting_violations[0].kind, kind, "kind {kind:?}");
    }
}

/// From codex audit round 7: pairing is **set membership**, exactly as `normalize.rs` does it, so a
/// duplicated `call_id` is closed in both directions. The generator only ever emits unique ids, so this
/// pins the semantics a counting or one-to-one refactor would silently break.
#[test]
fn validate_split_closure_uses_set_membership_for_duplicate_call_ids() {
    let two_calls_one_output = vec![
        user_message("turn 0"),
        function_call("x"),
        function_call("x"),
        function_call_output("x"),
        assistant_message("done"),
    ];
    let one_call_two_outputs = vec![
        user_message("turn 0"),
        function_call("x"),
        function_call_output("x"),
        function_call_output("x"),
        assistant_message("done"),
    ];

    for items in [&two_calls_one_output, &one_call_two_outputs] {
        assert_eq!(
            validate_split_closure(items, items.len()),
            ClosureReport::default(),
            "duplicate call ids must not be reported: {items:?}"
        );
        // And the real normalizer agrees: nothing is fabricated and nothing is dropped.
        assert_eq!(&for_prompt_roundtrip(items), items);
    }

    // Cut *between* the duplicated calls: the first copy loses the only output, the second keeps it.
    // The cut index differs per fixture, so a shared index would be right for the wrong reason.
    let report = validate_split_closure(&two_calls_one_output, 2);
    assert_eq!(
        induced_kinds(&report),
        vec![ClosureViolationKind::DanglingFunctionCall]
    );
    assert_eq!(report.induced[0].index, 1);
    assert_eq!(report.induced[0].side, SplitSide::Older);

    // Cut *between* the duplicated outputs: the second copy loses the only call.
    let report = validate_split_closure(&one_call_two_outputs, 3);
    assert_eq!(
        induced_kinds(&report),
        vec![ClosureViolationKind::OrphanFunctionCallOutput]
    );
    assert_eq!(report.induced[0].index, 3);
    assert_eq!(report.induced[0].side, SplitSide::Newer);
}

#[test]
fn validate_split_closure_clamps_out_of_range_index() {
    let items = vec![user_message("hi"), assistant_message("there")];

    assert!(validate_split_closure(&items, usize::MAX).is_closed());
}

#[test]
fn validate_split_closure_treats_metadata_less_items_as_inert() {
    let items = vec![
        additional_tools(),
        ResponseItem::Other,
        user_message("hi"),
        ResponseItem::CompactionTrigger {},
        assistant_message("there"),
        additional_tools(),
    ];

    for split_index in 0..=items.len() {
        let report = validate_split_closure(&items, split_index);
        assert!(
            report.is_closed() && report.preexisting.is_empty(),
            "metadata-less items must never participate in pairing (split {split_index})"
        );
    }
}

/// `property_selection_matches_an_independent_nearest_safe_candidate_oracle` asserts that the aimed
/// candidate is the unique token-weighted nearest one. That needs candidate prefix sums to be
/// strictly increasing. For two candidates `c1 < c2`, `prefix[c2] - prefix[c1]` always includes
/// `items[c1]`, so it is enough that every item a candidate can *point at* costs at least one token.
/// A candidate points at either a turn boundary or a pre-turn context item, and both are always
/// `Message` or `AgentMessage`.
///
/// Note the contrast: a `Reasoning` item is **not** guaranteed to cost anything, because
/// `history::estimate_reasoning_length` subtracts 650 from its estimate, so short encrypted reasoning
/// estimates zero. That is pre-existing and harmless here precisely because reasoning can never be a
/// candidate position.
#[test]
fn every_candidate_position_item_costs_at_least_one_token() {
    for item in [
        user_message(""),
        assistant_message(""),
        developer_message(""),
        contextual_developer_message(),
        contextual_user_message(),
        mixed_developer_bundle(),
        agent_message(""),
    ] {
        assert!(
            estimate_item_token_count(&item) > 0,
            "zero-cost candidate position would allow token-target ties: {item:?}"
        );
    }
    // Documents the pre-existing zero-cost case, so a future change to the estimator that made
    // reasoning a candidate position would have to revisit the tie argument above.
    assert_eq!(estimate_item_token_count(&reasoning("")), 0);
}

#[test]
fn violation_kinds_match_the_normalize_assertion_sites() {
    assert!(!ClosureViolationKind::DanglingFunctionCall.aborts_debug_builds());
    assert!(!ClosureViolationKind::DanglingToolSearchCall.aborts_debug_builds());
    assert!(ClosureViolationKind::DanglingCustomToolCall.aborts_debug_builds());
    assert!(ClosureViolationKind::DanglingLocalShellCall.aborts_debug_builds());
    assert!(ClosureViolationKind::OrphanFunctionCallOutput.aborts_debug_builds());
    assert!(ClosureViolationKind::OrphanCustomToolCallOutput.aborts_debug_builds());
    assert!(ClosureViolationKind::OrphanToolSearchOutput.aborts_debug_builds());
}

// ---------------------------------------------------------------------------
// split selection
// ---------------------------------------------------------------------------

/// Three equal turns. The token-weighted midpoint sits inside turn 2, but the split must land on the
/// boundary that starts turn 2, never on `len / 2`.
fn balanced_history(turns: u8) -> Vec<ResponseItem> {
    // The session-prefix developer message shares turn 0's id, exactly as `build_initial_context`
    // stamps it (`session/mod.rs:3483-3486`), so the first boundary snaps to index 0 and is not an
    // interior candidate. Compacting only the session prefix is never useful anyway.
    let mut items = vec![stamped(developer_message("You are Codex."), "turn-0")];
    for turn in 0..turns {
        let turn_id = format!("turn-{turn}");
        items.push(stamped(user_message(&format!("turn {turn}")), &turn_id));
        items.push(stamped(reasoning(&format!("think {turn}")), &turn_id));
        items.push(stamped(function_call(&format!("c{turn}")), &turn_id));
        items.push(stamped(function_call_output(&format!("c{turn}")), &turn_id));
        items.push(stamped(
            assistant_message(&format!("done {turn}")),
            &turn_id,
        ));
    }
    items
}

#[test]
fn select_compaction_split_lands_on_a_turn_boundary_not_the_midpoint() {
    let items = balanced_history(4);
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect("four balanced turns admit a safe split");

    // Turn boundaries are at 1, 6, 11 and 16; boundary 1 snaps into the session prefix, so the
    // interior candidates are 6, 11 and 16. The midpoint by index is 10, which is mid-turn.
    assert_eq!(turn_boundary_indices(&items), vec![1, 6, 11, 16]);
    assert_eq!(split_candidates(&items), vec![6, 11, 16]);
    assert_eq!(split.split_index, 11);
    assert_ne!(split.split_index, items.len() / 2);
    assert_eq!(split.snapped_outward_by, 0);
    assert!(split.preexisting_violations.is_empty());
    assert_derived_from_turn_boundary(&items, split.split_index);
    assert!(validate_split_closure(&items, split.split_index).is_closed());
}

#[test]
fn select_compaction_split_reports_token_estimates_that_sum_to_the_total() {
    let items = balanced_history(4);
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect("four balanced turns admit a safe split");
    let total: i64 = items
        .iter()
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add);

    assert_eq!(
        split
            .older_token_estimate
            .saturating_add(split.newer_token_estimate),
        total
    );
}

#[test]
fn select_compaction_split_target_fraction_moves_the_boundary() {
    let items = balanced_history(4);
    let early =
        select_compaction_split(&items, fraction_targeting(&items, 6)).expect("early split");
    let middle = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("mid split");
    let late = select_compaction_split(&items, 0.95).expect("late split");

    assert_eq!(early.split_index, 6);
    assert_eq!(middle.split_index, 11);
    assert_eq!(late.split_index, 16);
}

/// A fraction so small that the nearest boundary is the first turn, whose cut collapses into the
/// session prefix. The conservative answer is a typed rejection: jumping to the next boundary would
/// compact an entire turn the caller did not ask for. From codex audit round 6.
#[test]
fn select_compaction_split_rejects_rather_than_compacting_more_than_asked() {
    let items = balanced_history(4);
    assert_eq!(turn_boundary_indices(&items), vec![1, 6, 11, 16]);

    let rejection = select_compaction_split(&items, 0.0).expect_err("boundary 1 has no usable cut");
    match rejection {
        SplitRejection::NoSafeBoundary {
            candidates_tried,
            unusable_candidates,
            nearest_induced,
        } => {
            assert_eq!(candidates_tried, 0);
            assert_eq!(unusable_candidates, 1);
            assert!(nearest_induced.is_empty());
        }
        other => panic!("expected NoSafeBoundary, got {other:?}"),
    }
}

#[test]
fn select_compaction_split_clamps_invalid_fractions() {
    let items = balanced_history(4);
    let baseline = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect("baseline split")
        .split_index;

    assert_eq!(
        select_compaction_split(&items, f64::NAN)
            .expect("NaN falls back to the default fraction")
            .split_index,
        baseline
    );
    // Clamped to 0.0, which targets the first boundary; see
    // `select_compaction_split_rejects_rather_than_compacting_more_than_asked`.
    assert_eq!(
        select_compaction_split(&items, -5.0),
        select_compaction_split(&items, 0.0)
    );
    assert_eq!(
        select_compaction_split(&items, f64::INFINITY)
            .expect("infinity falls back to the default fraction")
            .split_index,
        baseline
    );
    assert_eq!(
        select_compaction_split(&items, 5.0)
            .expect("above one clamps to 1.0")
            .split_index,
        16
    );
}

/// A tool call started in turn 2 and only answered in turn 3, which is what a deferred tool future
/// produces. The boundary nearest the midpoint would orphan it, so the split must snap outward to
/// the earlier boundary and compact *less*, never drop the call or its output.
#[test]
fn select_compaction_split_snaps_outward_across_a_cross_turn_pair() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(assistant_message("done 0"), "turn-0"),
        stamped(user_message("turn 1"), "turn-1"),
        stamped(function_call("straddle"), "turn-1"),
        stamped(assistant_message("still working"), "turn-1"),
        stamped(user_message("turn 2"), "turn-2"),
        stamped(function_call_output("straddle"), "turn-2"),
        stamped(assistant_message("done 2"), "turn-2"),
    ];
    assert_eq!(split_candidates(&items), vec![3, 6]);

    // Aim the target exactly at candidate 6, which straddles the pair.
    assert!(!validate_split_closure(&items, 6).is_closed());
    let split = select_compaction_split(&items, fraction_targeting(&items, 6))
        .expect("an earlier boundary is safe");

    assert_eq!(split.target_boundary, 6);
    assert_eq!(split.split_index, 3, "snapped outward, compacting less");
    assert_eq!(split.snapped_outward_by, 1);
    assert!(split.split_index < split.target_boundary);
    // Nothing was dropped.
    assert_eq!(
        [&items[..split.split_index], &items[split.split_index..]].concat(),
        items
    );
    // And both halves survive real normalization untouched.
    assert_eq!(
        for_prompt_roundtrip(&items[..split.split_index]),
        items[..split.split_index].to_vec()
    );
    assert_eq!(
        for_prompt_roundtrip(&items[split.split_index..]),
        items[split.split_index..].to_vec()
    );
}

#[test]
fn select_compaction_split_keeps_pre_turn_context_with_its_turn() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(assistant_message("done 0"), "turn-0"),
        stamped(contextual_developer_message(), "turn-1"),
        stamped(contextual_user_message(), "turn-1"),
        stamped(user_message("turn 1"), "turn-1"),
        stamped(assistant_message("done 1"), "turn-1"),
        stamped(user_message("turn 2"), "turn-2"),
        stamped(assistant_message("done 2"), "turn-2"),
    ];

    // Boundary 5 snaps back over indices 4 and 3, which describe turn 1.
    assert_eq!(turn_boundary_indices(&items), vec![1, 5, 7]);
    assert_eq!(split_candidates(&items), vec![3, 7]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 5)).expect("safe split");
    assert_eq!(split.split_index, 3);
    assert!(matches!(
        items[split.split_index],
        ResponseItem::Message { .. }
    ));
    assert_derived_from_turn_boundary(&items, split.split_index);
}

/// A mixed `build_initial_context` bundle is snapped over like any other pre-turn context item, so
/// its persistent developer text lands in the verbatim half instead of being summarized away.
#[test]
fn select_compaction_split_snaps_over_a_mixed_initial_context_bundle() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(assistant_message("done 0"), "turn-0"),
        stamped(mixed_developer_bundle(), "turn-1"),
        stamped(user_message("turn 1"), "turn-1"),
        stamped(assistant_message("done 1"), "turn-1"),
    ];

    assert_eq!(split_candidates(&items), vec![3]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 3)).expect("safe split");
    assert_eq!(split.split_index, 3);
    // The bundle keeps its position and stays verbatim; nothing was relocated.
    assert_eq!(
        items[split.split_index],
        stamped(mixed_developer_bundle(), "turn-1")
    );
    assert_eq!(
        [&items[..split.split_index], &items[split.split_index..]].concat(),
        items
    );
}

/// From codex audit round 2: a persistent developer message carrying no recognized contextual
/// prefix (the separate guardian-policy message, the multi-agent usage hint) must not be summarized
/// away, so the walk traverses any developer message rather than only contextual ones.
#[test]
fn select_compaction_split_snaps_over_a_persistent_developer_message() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(assistant_message("done 0"), "turn-0"),
        stamped(mixed_developer_bundle(), "turn-1"),
        stamped(developer_message("guardian policy"), "turn-1"),
        stamped(user_message("turn 1"), "turn-1"),
        stamped(assistant_message("done 1"), "turn-1"),
    ];

    // Boundary 5 snaps over 4 (persistent developer) and 3 (mixed bundle).
    assert_eq!(turn_boundary_indices(&items), vec![1, 5]);
    assert_eq!(split_candidates(&items), vec![3]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 5)).expect("safe split");
    assert_eq!(split.split_index, 3);
    assert_derived_from_turn_boundary(&items, split.split_index);
}

/// From codex audit round 4: mid-turn compaction stamps freshly built initial context with the
/// *compaction* turn and inserts it immediately above the last **retained** real user message
/// (`compact.rs:553-583`), which keeps its own older id or none at all. A rule that required matching
/// turn ids would summarize those exact developer instructions away, so ids are not consulted.
#[test]
fn select_compaction_split_snaps_over_reinjected_initial_context_with_a_foreign_turn_id() {
    let items = vec![
        stamped(user_message("older"), "turn-a"),
        stamped(assistant_message("done"), "turn-a"),
        stamped(mixed_developer_bundle(), "turn-c"),
        stamped(developer_message("guardian policy"), "turn-c"),
        // Retained from before compaction, so its id is older than the injected context's.
        stamped(user_message("last retained user"), "turn-b"),
        stamped(assistant_message("done b"), "turn-b"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![0, 4]);
    assert_eq!(split_candidates(&items), vec![2]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 4)).expect("safe split");
    assert_eq!(split.split_index, 2);
    // Both developer items stayed verbatim rather than being summarized away.
    assert_eq!(
        for_prompt_roundtrip(&items[split.split_index..]),
        items[split.split_index..].to_vec()
    );
}

/// From codex audit round 5, and the sharpest bug found in the whole task: snapping happens *before*
/// token targeting, so if the target were chosen among snapped indices, a boundary with a large
/// pre-turn context would have its snapped index pulled far from the target and a **later** boundary
/// would become the nearest one. The selector would then compact strictly more than asked, exactly in
/// the case snapping exists to protect.
///
/// Targeting therefore runs on turn boundaries and only the accepted boundary is snapped.
#[test]
fn select_compaction_split_targets_boundaries_so_snapping_cannot_retarget_later() {
    let bulk = "x".repeat(4000);
    let items = vec![
        stamped(user_message("older"), "turn-a"),
        stamped(assistant_message("done"), "turn-a"),
        stamped(
            developer_message(&format!("reinjected context {bulk}")),
            "turn-c",
        ),
        stamped(
            developer_message(&format!("guardian policy {bulk}")),
            "turn-c",
        ),
        stamped(user_message("retained user"), "turn-b"),
        stamped(assistant_message("ok"), "turn-b"),
        stamped(user_message("newest user"), "turn-d"),
        stamped(assistant_message("ok"), "turn-d"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![0, 4, 6]);
    assert_eq!(split_candidates(&items), vec![2, 6]);

    // The condition that made the bug fire: aiming at boundary 4, the *snapped* candidate 6 is nearer
    // to the target than the snapped candidate 2, because items 2 and 3 are large.
    let prefix = |end: usize| -> i64 {
        items
            .iter()
            .take(end)
            .map(estimate_item_token_count)
            .fold(0i64, i64::saturating_add)
    };
    let target = prefix(4);
    assert!(
        (prefix(6) - target).abs() < (prefix(2) - target).abs(),
        "fixture no longer reproduces the retargeting condition"
    );

    let split = select_compaction_split(&items, fraction_targeting(&items, 4)).expect("safe split");
    assert_eq!(split.target_boundary, 4);
    assert_eq!(
        split.split_index, 2,
        "must snap boundary 4 outward, not jump to 6"
    );
    // The exact developer instructions and the whole retained turn stayed verbatim.
    assert_eq!(
        for_prompt_roundtrip(&items[split.split_index..]),
        items[split.split_index..].to_vec()
    );
}

/// From codex audit round 6, the mirror image of round 5's finding at the other end: a boundary whose
/// snapped cut collapses to index 0 must still be *targetable*. Filtering it out before targeting hands
/// the target to a later boundary, which summarizes an entire extra turn plus the developer context.
#[test]
fn select_compaction_split_rejects_when_the_targeted_boundary_has_no_usable_cut() {
    let items = vec![
        stamped(developer_message("reinjected context"), "turn-c"),
        stamped(user_message("retained user"), "turn-b"),
        stamped(assistant_message("ok"), "turn-b"),
        stamped(user_message("newer turn"), "turn-d"),
        stamped(assistant_message("ok"), "turn-d"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![1, 3]);
    // Boundary 1 snaps into the developer item at 0, so its only cut would leave an empty older half.
    assert_eq!(split_candidates(&items), vec![3]);

    let rejection = select_compaction_split(&items, fraction_targeting(&items, 1))
        .expect_err("must not jump forward to boundary 3");
    match rejection {
        SplitRejection::NoSafeBoundary {
            candidates_tried,
            unusable_candidates,
            nearest_induced,
        } => {
            assert_eq!(candidates_tried, 0);
            assert_eq!(unusable_candidates, 1);
            assert!(nearest_induced.is_empty());
        }
        other => panic!("expected NoSafeBoundary, got {other:?}"),
    }

    // Aiming at boundary 3 is a different request and is still served.
    let split = select_compaction_split(&items, fraction_targeting(&items, 3)).expect("safe split");
    assert_eq!(split.target_boundary, 3);
    assert_eq!(split.split_index, 3);
}

/// The same rule with the retained user message carrying **no** turn id at all.
#[test]
fn select_compaction_split_snaps_over_reinjected_initial_context_above_an_unstamped_user() {
    let items = vec![
        stamped(user_message("older"), "turn-a"),
        stamped(assistant_message("done"), "turn-a"),
        stamped(mixed_developer_bundle(), "turn-c"),
        stamped(developer_message("guardian policy"), "turn-c"),
        user_message("last retained user"),
        assistant_message("done b"),
    ];

    assert_eq!(split_candidates(&items), vec![2]);
}

/// The accepted cost of ignoring turn ids, recorded so a future change cannot alter it silently.
///
/// A previous turn's trailing contextual items - skill and plugin injection recorded after the input
/// (`session/turn.rs:192-208`), or the interrupt marker (`tasks/mod.rs:97-105`, a contextual *user*
/// fragment) - are carried into the verbatim half. Nothing is reordered or dropped and closure is
/// unaffected; the split simply compacts less.
#[test]
fn select_compaction_split_carries_a_previous_turns_trailing_contextual_items_verbatim() {
    let items = vec![
        stamped(user_message("turn 0"), "turn-A"),
        stamped(contextual_user_message(), "turn-A"),
        stamped(contextual_developer_message(), "turn-A"),
        stamped(user_message("turn 1"), "turn-B"),
        stamped(assistant_message("done 1"), "turn-B"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![0, 3]);
    // Boundary 3 walks back over turn A's trailing contextual items: accepted, compacts less.
    assert_eq!(split_candidates(&items), vec![1]);
    let split = select_compaction_split(&items, fraction_targeting(&items, 3)).expect("safe split");
    assert_eq!(split.split_index, 1);
    assert_eq!(
        [&items[..split.split_index], &items[split.split_index..]].concat(),
        items
    );
}

/// Same accepted cost for steered inputs, which share one turn's `sub_id`
/// (two boundaries with the same id).
#[test]
fn select_compaction_split_handles_two_boundaries_sharing_one_turn_id() {
    let items = vec![
        stamped(user_message("steer 1"), "turn-A"),
        stamped(developer_message("hook context for steer 1"), "turn-A"),
        stamped(user_message("steer 2"), "turn-A"),
        stamped(assistant_message("done"), "turn-A"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![0, 2]);
    assert_eq!(split_candidates(&items), vec![1]);
}

/// A legacy rollout reconstructed through `ContextManager::record_items`
/// (`rollout_reconstruction.rs:328`) carries no turn ids at all. The content-only rule behaves
/// identically there, which is the point of not depending on metadata.
#[test]
fn select_compaction_split_snaps_over_unstamped_pre_turn_developer_messages() {
    let items = vec![
        developer_message("You are Codex."),
        user_message("turn 0"),
        assistant_message("done 0"),
        mixed_developer_bundle(),
        developer_message("guardian policy"),
        user_message("turn 1"),
        assistant_message("done 1"),
    ];
    assert!(items.iter().all(|item| item.turn_id().is_none()));

    // Boundary 5 snaps over 4 and 3; boundary 1 snaps into the unstamped session prefix at 0.
    assert_eq!(turn_boundary_indices(&items), vec![1, 5]);
    assert_eq!(split_candidates(&items), vec![3]);
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("safe split");
    assert_eq!(split.split_index, 3);
    // Both persistent developer items stayed verbatim.
    assert_eq!(
        for_prompt_roundtrip(&items[split.split_index..]),
        items[split.split_index..].to_vec()
    );
}

/// From codex audit round 2: candidate safety is not monotone. Candidate 3 is safe while both 1 and
/// 6 are unsafe, so the snap must find 3 rather than walk past it to a rejection.
#[test]
fn select_compaction_split_snaps_past_an_unsafe_target_to_a_non_monotone_safe_candidate() {
    let items = vec![
        function_call("a"),
        user_message("turn 0"),
        function_call_output("a"),
        user_message("turn 1"),
        function_call("b"),
        assistant_message("working"),
        user_message("turn 2"),
        function_call_output("b"),
    ];

    assert_eq!(split_candidates(&items), vec![1, 3, 6]);
    assert!(!validate_split_closure(&items, 1).is_closed());
    assert!(validate_split_closure(&items, 3).is_closed());
    assert!(!validate_split_closure(&items, 6).is_closed());

    let split = select_compaction_split(&items, fraction_targeting(&items, 6)).expect("3 is safe");
    assert_eq!(split.target_boundary, 6);
    assert_eq!(split.split_index, 3);
    assert_eq!(split.snapped_outward_by, 1);
}

/// The named gap from codex audit round 1: a pre-existing dangling `ToolSearchCall` must be reported
/// as pre-existing and must not make every candidate look unsafe.
#[test]
fn select_compaction_split_reports_a_preexisting_dangling_tool_search_call() {
    let items = vec![
        developer_message("You are Codex."),
        user_message("turn 0"),
        assistant_message("done 0"),
        user_message("turn 1"),
        tool_search_call("x"),
        assistant_message("done 1"),
    ];
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("safe split");

    assert_eq!(split.split_index, 3);
    assert_eq!(split.preexisting_violations.len(), 1);
    assert_eq!(
        split.preexisting_violations[0].kind,
        ClosureViolationKind::DanglingToolSearchCall
    );
    assert_eq!(split.preexisting_violations[0].index, 4);
    assert_eq!(split.preexisting_violations[0].side, SplitSide::Newer);

    // `for_prompt` fabricates the empty tool search output on the side that already held the call.
    let older = &items[..split.split_index];
    assert_eq!(for_prompt_roundtrip(older), older.to_vec());
    let newer = &items[split.split_index..];
    assert_eq!(for_prompt_roundtrip(newer).len(), newer.len() + 1);
}

#[test]
fn select_compaction_split_treats_agent_messages_as_turn_boundaries() {
    let items = vec![
        stamped(developer_message("You are Codex."), "turn-0"),
        stamped(user_message("turn 0"), "turn-0"),
        stamped(assistant_message("done 0"), "turn-0"),
        stamped(agent_message("delegate this"), "turn-1"),
        stamped(assistant_message("done 1"), "turn-1"),
    ];

    assert_eq!(turn_boundary_indices(&items), vec![1, 3]);
    // Boundary 1 snaps into the session prefix it shares a turn with; boundary 3 stands alone.
    assert_eq!(split_candidates(&items), vec![3]);
}

#[test]
fn select_compaction_split_never_selects_a_metadata_less_item() {
    let items = vec![
        additional_tools(),
        developer_message("You are Codex."),
        user_message("turn 0"),
        ResponseItem::Other,
        assistant_message("done 0"),
        additional_tools(),
        user_message("turn 1"),
        ResponseItem::CompactionTrigger {},
        assistant_message("done 1"),
    ];
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("safe split");

    assert!(!is_turn_metadata_less(&items[split.split_index]));
    assert_eq!(split.split_index, 6);
}

#[test]
fn select_compaction_split_reports_preexisting_violations_without_blaming_the_split() {
    let items = vec![
        developer_message("You are Codex."),
        user_message("turn 0"),
        // Interrupted: no output was ever recorded for this call.
        function_call("interrupted"),
        user_message("turn 1"),
        function_call("c1"),
        function_call_output("c1"),
        assistant_message("done 1"),
    ];
    let split = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION).expect("safe split");

    assert_eq!(split.split_index, 3);
    assert_eq!(split.preexisting_violations.len(), 1);
    assert_eq!(
        split.preexisting_violations[0].kind,
        ClosureViolationKind::DanglingFunctionCall
    );
    assert_eq!(split.preexisting_violations[0].side, SplitSide::Older);

    // The pre-existing dangling call is repaired by `for_prompt` exactly as it is today, and the
    // repair stays inside the older half.
    let older = &items[..split.split_index];
    assert_eq!(for_prompt_roundtrip(older).len(), older.len() + 1);
    let newer = &items[split.split_index..];
    assert_eq!(for_prompt_roundtrip(newer), newer.to_vec());
}

// ---------------------------------------------------------------------------
// rejections
// ---------------------------------------------------------------------------

#[test]
fn select_compaction_split_rejects_an_empty_history() {
    assert_eq!(
        select_compaction_split(&[], DEFAULT_SPLIT_TARGET_FRACTION),
        Err(SplitRejection::EmptyHistory)
    );
}

#[test]
fn select_compaction_split_rejects_a_history_without_turn_boundaries() {
    let items = vec![
        developer_message("You are Codex."),
        contextual_user_message(),
        assistant_message("done"),
    ];

    assert_eq!(
        select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION),
        Err(SplitRejection::NoTurnBoundary)
    );
}

#[test]
fn select_compaction_split_rejects_when_every_boundary_is_at_the_head() {
    let items = vec![user_message("only turn"), assistant_message("done")];

    assert_eq!(
        select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION),
        Err(SplitRejection::NoInteriorTurnBoundary { boundaries: 1 })
    );
}

#[test]
fn select_compaction_split_rejects_when_no_boundary_is_safe() {
    // The history starts at a turn boundary, so index 0 is not an interior candidate and the only
    // remaining candidate straddles the pair. Snapping outward has nowhere left to go.
    let items = vec![
        user_message("turn 0"),
        function_call("straddle"),
        user_message("turn 1"),
        function_call_output("straddle"),
    ];

    assert_eq!(split_candidates(&items), vec![2]);
    let rejection = select_compaction_split(&items, DEFAULT_SPLIT_TARGET_FRACTION)
        .expect_err("no safe boundary exists");
    match rejection {
        SplitRejection::NoSafeBoundary {
            candidates_tried,
            unusable_candidates,
            nearest_induced,
        } => {
            assert_eq!(candidates_tried, 1);
            // Boundary 0's cut collapses to index 0, so it was skipped rather than validated.
            assert_eq!(unusable_candidates, 1);
            assert_eq!(
                nearest_induced
                    .iter()
                    .map(|violation| violation.kind)
                    .collect::<Vec<_>>(),
                vec![
                    ClosureViolationKind::DanglingFunctionCall,
                    ClosureViolationKind::OrphanFunctionCallOutput,
                ]
            );
        }
        other => panic!("expected NoSafeBoundary, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// property tests
// ---------------------------------------------------------------------------

/// SplitMix64. Deterministic and dependency-free, so a failing case is reproducible from its seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// Whether the generator may leave a call unanswered, reproducing an interrupted turn.
///
/// Only `FunctionCall` and `ToolSearchCall` are ever left dangling: those are the two kinds
/// `normalize::ensure_call_outputs_present` repairs without asserting, so the generated histories
/// stay safe to push through `for_prompt` in a debug build. The asserting kinds are covered by the
/// unit tests above, which never call `for_prompt`.
struct GeneratorConfig {
    allow_preexisting_dangling_calls: bool,
}

/// A generated history plus an **independent** record of the unpaired calls the generator planted.
///
/// `planted_dangling` is built by the generator at the moment it drops an output, not by asking
/// `validate_split_closure`. It is the oracle the pre-existing-violation property tests compare
/// against, so the classifier is never its own expectation.
struct GeneratedHistory {
    items: Vec<ResponseItem>,
    /// `(index in items, kind, call_id)` for every call the generator deliberately left unanswered.
    planted_dangling: Vec<(usize, ClosureViolationKind, String)>,
    /// `(first pre-turn item index, boundary index)` for every turn the generator gave pre-turn
    /// context items. Recorded independently of any production helper, so it can assert that a split
    /// never bisects such a run and never leaves one in the summarized half while its turn stays
    /// verbatim.
    planted_pre_turn_runs: Vec<(usize, usize)>,
}

fn generate_history(rng: &mut Rng, config: &GeneratorConfig) -> GeneratedHistory {
    let mut items = vec![developer_message("You are Codex.")];
    let mut planted_dangling: Vec<(usize, ClosureViolationKind, String)> = Vec::new();
    let mut planted_pre_turn_runs: Vec<(usize, usize)> = Vec::new();
    if rng.chance(30) {
        items.push(additional_tools());
    }
    let mut next_call = 0u64;
    // Outputs whose calls have already been emitted but which have not been written yet. Carrying
    // these across a turn boundary reproduces a deferred tool future resolving in a later turn.
    let mut pending: Vec<ResponseItem> = Vec::new();

    let turns = 2 + rng.below(6);
    for turn in 0..turns {
        // Items are accumulated per turn and stamped with that turn's id, mirroring
        // `Session::prepare_conversation_items_for_history` (`session/mod.rs:2813-2816`). Pre-turn
        // context items share the id of the user boundary that follows them, which is what the real
        // recording order produces.
        let turn_id = format!("turn-{turn}");
        let mut turn_items: Vec<ResponseItem> = Vec::new();
        let pre_turn_run_start = items.len();
        for _ in 0..rng.below(3) {
            match rng.below(3) {
                0 => turn_items.push(contextual_developer_message()),
                1 => turn_items.push(contextual_user_message()),
                // A persistent developer message with no recognized contextual prefix: the separate
                // guardian-policy message / multi-agent usage hint shape. Only its turn id ties it
                // to the following boundary.
                _ => turn_items.push(developer_message(&format!("persistent policy {turn}"))),
            }
        }
        if !turn_items.is_empty() {
            planted_pre_turn_runs.push((pre_turn_run_start, items.len() + turn_items.len()));
        }
        if rng.chance(15) {
            turn_items.push(agent_message(&format!("delegate {turn}")));
        } else {
            turn_items.push(user_message(&format!("turn {turn}")));
        }

        for _ in 0..rng.below(5) {
            if rng.chance(55) {
                turn_items.push(reasoning(&format!("think {turn}")));
            }
            if rng.chance(10) {
                turn_items.push(match rng.below(3) {
                    0 => additional_tools(),
                    1 => ResponseItem::Other,
                    _ => ResponseItem::CompactionTrigger {},
                });
            }
            next_call += 1;
            let call_id = format!("call-{next_call}");
            // `droppable` marks the two kinds `normalize::ensure_call_outputs_present` repairs
            // without asserting, so a planted dangling call keeps `for_prompt` panic-free.
            let (call, output, droppable) = match rng.below(7) {
                0 => (
                    function_call(&call_id),
                    Some(function_call_output(&call_id)),
                    Some(ClosureViolationKind::DanglingFunctionCall),
                ),
                1 => (
                    local_shell_call(&call_id),
                    Some(function_call_output(&call_id)),
                    None,
                ),
                2 => (
                    custom_tool_call(&call_id),
                    Some(custom_tool_call_output(&call_id)),
                    None,
                ),
                3 => (
                    tool_search_call(&call_id),
                    Some(tool_search_output(Some(&call_id), "client")),
                    Some(ClosureViolationKind::DanglingToolSearchCall),
                ),
                4 => (tool_search_output(None, "server"), None, None),
                5 => (web_search_call(), None, None),
                _ => (assistant_message(&format!("note {turn}")), None, None),
            };
            let call_index = items.len() + turn_items.len();
            turn_items.push(call);
            let Some(output) = output else {
                continue;
            };
            if let Some(kind) = droppable
                && config.allow_preexisting_dangling_calls
                && rng.chance(12)
            {
                // Interrupted before the output was recorded.
                planted_dangling.push((call_index, kind, call_id));
                continue;
            }
            if rng.chance(30) {
                pending.push(stamped(output, &turn_id));
            } else {
                turn_items.push(output);
            }
        }

        if rng.chance(80) {
            turn_items.push(assistant_message(&format!("done {turn}")));
        }
        // Usually flush inside the turn; sometimes let it straddle the next boundary. Already-stamped
        // deferrals keep their own turn id, because `set_turn_id_if_missing` never overwrites.
        if !rng.chance(15) {
            turn_items.append(&mut pending);
        }
        for item in &mut turn_items {
            item.set_turn_id_if_missing(&turn_id);
        }
        items.append(&mut turn_items);
    }
    // Never leave the generator's own deferrals dangling; only deliberate drops do that.
    items.append(&mut pending);
    GeneratedHistory {
        items,
        planted_dangling,
        planted_pre_turn_runs,
    }
}

/// A split must either take a whole planted pre-turn run into the verbatim half or leave the whole
/// run, together with its turn, in the summarized half. Bisecting the run, or leaving the run behind
/// while its turn stays verbatim, is the failure the round-1 and round-2 audit findings were about.
///
/// The expectation comes from the generator's own record, not from `snap_over_pre_turn_context`.
fn assert_planted_pre_turn_runs_stay_with_their_turn(
    generated: &GeneratedHistory,
    split_index: usize,
) {
    for (run_start, boundary) in generated.planted_pre_turn_runs.iter().copied() {
        assert!(
            !(run_start < split_index && split_index <= boundary),
            "split {split_index} separated the pre-turn run {run_start}..{boundary} from its turn"
        );
    }
}

/// Assertions that must hold for every accepted split of every generated history.
fn assert_split_invariants(items: &[ResponseItem], split: &HistorySplit) {
    let split_index = split.split_index;
    assert!(
        split_index > 0 && split_index < items.len(),
        "split {split_index} must leave both halves non-empty (len {})",
        items.len()
    );
    let report = validate_split_closure(items, split_index);
    assert!(
        report.is_closed(),
        "accepted split {split_index} induced {:?}",
        report.induced
    );
    assert_eq!(
        report.preexisting, split.preexisting_violations,
        "reported pre-existing violations must match a fresh validation"
    );
    assert_eq!(
        [&items[..split_index], &items[split_index..]].concat(),
        items.to_vec(),
        "the split must not drop or reorder any item"
    );
    assert!(
        split_index <= split.target_boundary,
        "snapping must move outward (compact less), not inward"
    );
    assert!(!is_turn_metadata_less(&items[split_index]));
    assert_derived_from_turn_boundary(items, split_index);
    assert_reasoning_travels_with_its_turn(items, split_index);

    let total: i64 = items
        .iter()
        .map(estimate_item_token_count)
        .fold(0i64, i64::saturating_add);
    assert_eq!(
        split
            .older_token_estimate
            .saturating_add(split.newer_token_estimate),
        total
    );

    // Every pre-existing violation the generator planted must be one of the non-asserting kinds, so
    // the `for_prompt` calls below cannot panic for a reason this module is not responsible for.
    for violation in &report.preexisting {
        assert!(
            !violation.kind.aborts_debug_builds(),
            "generator planted an asserting pre-existing violation: {violation:?}"
        );
    }

    // The real proof. In a debug build `error_or_panic` panics, so surviving these two calls means
    // neither half trips the pairing invariant. The length assertions additionally prove that the
    // only repairs performed were for pre-existing dangling calls, and that they stayed on the side
    // that already held them.
    let older = &items[..split_index];
    let newer = &items[split_index..];
    let older_preexisting = report
        .preexisting
        .iter()
        .filter(|violation| violation.side == SplitSide::Older)
        .count();
    let newer_preexisting = report.preexisting.len() - older_preexisting;
    assert_eq!(
        for_prompt_roundtrip(older).len(),
        older.len() + older_preexisting,
        "normalization changed the older half beyond repairing pre-existing dangling calls"
    );
    assert_eq!(
        for_prompt_roundtrip(newer).len(),
        newer.len() + newer_preexisting,
        "normalization changed the newer half beyond repairing pre-existing dangling calls"
    );
    if report.preexisting.is_empty() {
        assert_eq!(for_prompt_roundtrip(older), older.to_vec());
        assert_eq!(for_prompt_roundtrip(newer), newer.to_vec());
    }
}

/// Compare the reported pre-existing violations against what the generator recorded at the moment it
/// dropped an output.
///
/// This is the independent oracle: it never consults `validate_split_closure`. The generator's
/// deliberate drops are its only source of unpaired items, because every deferred output is flushed
/// before it returns.
fn assert_planted_dangling_matches(
    generated: &GeneratedHistory,
    reported: &[ClosureViolation],
    seed: u64,
) {
    let expected: Vec<(usize, ClosureViolationKind, &str)> = generated
        .planted_dangling
        .iter()
        .map(|(index, kind, call_id)| (*index, *kind, call_id.as_str()))
        .collect();
    let actual: Vec<(usize, ClosureViolationKind, &str)> = reported
        .iter()
        .map(|violation| (violation.index, violation.kind, violation.call_id.as_str()))
        .collect();
    assert_eq!(
        actual, expected,
        "seed {seed}: reported pre-existing violations do not match what the generator planted"
    );
}

/// Returns `Some(snapped_outward_by)` when a split was accepted, `None` on a typed rejection.
fn run_property_case(seed: u64, config: &GeneratorConfig, fraction: f64) -> Option<usize> {
    let mut rng = Rng::new(seed);
    let generated = generate_history(&mut rng, config);
    let items = generated.items.as_slice();
    match select_compaction_split(items, fraction) {
        Ok(split) => {
            assert_planted_pre_turn_runs_stay_with_their_turn(&generated, split.split_index);
            assert_split_invariants(items, &split);
            assert_planted_dangling_matches(&generated, &split.preexisting_violations, seed);
            Some(split.snapped_outward_by)
        }
        Err(SplitRejection::NoSafeBoundary { .. }) => None,
        Err(other) => panic!("seed {seed}: unexpected rejection {other:?} for {items:?}"),
    }
}

#[test]
fn property_every_accepted_split_is_closed_and_normalizes_unchanged() {
    let config = GeneratorConfig {
        allow_preexisting_dangling_calls: false,
    };
    let fractions = [0.25f64, DEFAULT_SPLIT_TARGET_FRACTION, 0.75];
    let mut accepted = 0usize;
    let mut snapped = 0usize;
    let mut cases = 0usize;
    for seed in 0..400u64 {
        let fraction = fractions[(seed as usize) % fractions.len()];
        cases += 1;
        if let Some(snapped_outward_by) = run_property_case(
            seed.wrapping_mul(0x51_7C_C1_B7_27_22_0A_95),
            &config,
            fraction,
        ) {
            accepted += 1;
            if snapped_outward_by > 0 {
                snapped += 1;
            }
        }
    }

    assert_eq!(cases, 400);
    // Guards against a vacuous pass where every case bailed out with a typed rejection.
    assert!(
        accepted * 10 >= cases * 6,
        "only {accepted}/{cases} cases produced a split; the suite would be near-vacuous"
    );
    // Guards against the snap-outward branch never being exercised by the generator.
    assert!(
        snapped > 0,
        "no generated case ever snapped outward, so that branch is untested here"
    );
}

#[test]
fn property_preexisting_dangling_calls_are_reported_not_induced() {
    let config = GeneratorConfig {
        allow_preexisting_dangling_calls: true,
    };
    let mut accepted = 0usize;
    let mut with_preexisting = 0usize;
    let cases = 300usize;
    for seed in 0..cases as u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        let generated = generate_history(&mut rng, &config);
        let items = generated.items.as_slice();
        match select_compaction_split(items, DEFAULT_SPLIT_TARGET_FRACTION) {
            Ok(split) => {
                assert_planted_pre_turn_runs_stay_with_their_turn(&generated, split.split_index);
                assert_split_invariants(items, &split);
                // The expectation comes from the generator's own record, not from the classifier.
                assert_planted_dangling_matches(&generated, &split.preexisting_violations, seed);
                if !split.preexisting_violations.is_empty() {
                    with_preexisting += 1;
                }
                accepted += 1;
            }
            Err(SplitRejection::NoSafeBoundary { .. }) => {}
            Err(other) => panic!("seed {seed}: unexpected rejection {other:?}"),
        }
    }

    assert!(
        accepted * 10 >= cases * 5,
        "only {accepted}/{cases} cases produced a split"
    );
    assert!(
        with_preexisting > 0,
        "the generator never planted a pre-existing dangling call, so this test proves nothing"
    );
}

/// Independent re-derivation of the snap-outward rule, indexed by the **turn boundary** the selector
/// targeted: the chosen split must be the snapped index of the nearest boundary at or before it that
/// induces no violation, and `snapped_outward_by` must be how many were skipped to get there.
fn expected_snap_from_target(
    items: &[ResponseItem],
    boundaries: &[usize],
    target_boundary: usize,
) -> Option<(usize, usize)> {
    let target_position = boundaries
        .iter()
        .position(|boundary| *boundary == target_boundary)?;
    let mut skipped = 0usize;
    for boundary in boundaries.get(..=target_position)?.iter().copied().rev() {
        let split_index = snapped_split_index(items, boundary);
        // A cut that collapses to 0 would leave an empty older half. It is skipped without being
        // counted as a rejected candidate, matching `snapped_outward_by`.
        if split_index == 0 {
            continue;
        }
        if validate_split_closure(items, split_index).is_closed() {
            return Some((split_index, skipped));
        }
        skipped += 1;
    }
    None
}

/// Independent re-derivation of the snap itself, from the item shape rather than from
/// `snap_over_pre_turn_context`.
fn snapped_split_index(items: &[ResponseItem], boundary: usize) -> usize {
    let mut split_index = boundary;
    while split_index > 0 && is_pre_turn_context_item(&items[split_index - 1]) {
        split_index -= 1;
    }
    split_index
}

#[test]
fn property_selection_matches_an_independent_nearest_safe_candidate_oracle() {
    let config = GeneratorConfig {
        allow_preexisting_dangling_calls: true,
    };
    let mut checked = 0usize;
    let mut snapped = 0usize;
    let mut rejected = 0usize;
    for seed in 0..200u64 {
        let mut rng = Rng::new(seed ^ 0xA076_1D64_78BD_642F);
        let generated = generate_history(&mut rng, &config);
        let items = generated.items.as_slice();
        let whole = validate_split_closure(items, items.len());
        assert!(
            whole.induced.is_empty(),
            "seed {seed}: a full-length split cannot induce anything"
        );
        // Every boundary is targetable, including ones whose snapped cut collapses to 0. Filtering
        // those out here would repeat the round-6 bug inside the oracle and hide it.
        let boundaries = turn_boundary_indices(items);

        for boundary in boundaries.iter().copied() {
            let candidate = snapped_split_index(items, boundary);
            if candidate > 0 {
                // Splitting never fixes a pre-existing violation, so the pre-existing set is stable
                // across every candidate.
                let report = validate_split_closure(items, candidate);
                assert_eq!(
                    report.preexisting.len(),
                    whole.preexisting.len(),
                    "seed {seed}: candidate {candidate} changed the pre-existing set"
                );
                for violation in report.induced.iter().chain(report.preexisting.iter()) {
                    assert!(violation.index < items.len());
                }
            }

            // Aim the selector exactly at this boundary. Boundary prefix sums strictly increase (see
            // `every_candidate_position_item_costs_at_least_one_token`), so the aimed boundary is the
            // unique nearest one; asserting that also covers the token-targeting step and proves
            // targeting runs on boundaries rather than on snapped indices.
            let fraction = fraction_targeting(items, boundary);
            match select_compaction_split(items, fraction) {
                Ok(split) => {
                    assert_eq!(
                        split.target_boundary, boundary,
                        "seed {seed}: token targeting picked {} instead of the aimed {boundary}",
                        split.target_boundary
                    );
                    assert_eq!(
                        Some((split.split_index, split.snapped_outward_by)),
                        expected_snap_from_target(items, &boundaries, boundary),
                        "seed {seed}: aimed at boundary {boundary}"
                    );
                    checked += 1;
                    if split.snapped_outward_by > 0 {
                        snapped += 1;
                    }
                }
                Err(SplitRejection::NoSafeBoundary {
                    candidates_tried,
                    unusable_candidates,
                    ..
                }) => {
                    // Candidate safety is not monotone, so checking only the earliest candidate
                    // would let a selector that skipped a safe one pass. Assert every usable
                    // candidate at or before the target is genuinely unsafe, and that the two counts
                    // account for every boundary at or before it.
                    let at_or_before: Vec<usize> = boundaries
                        .iter()
                        .copied()
                        .take_while(|b| *b <= boundary)
                        .collect();
                    let mut expected_unusable = 0usize;
                    let mut expected_tried = 0usize;
                    for earlier in at_or_before {
                        let split_index = snapped_split_index(items, earlier);
                        if split_index == 0 {
                            expected_unusable += 1;
                            continue;
                        }
                        assert!(
                            !validate_split_closure(items, split_index).is_closed(),
                            "seed {seed}: rejected while boundary {earlier} (at or before the \
                             target {boundary}) was safe"
                        );
                        expected_tried += 1;
                    }
                    assert_eq!(
                        (candidates_tried, unusable_candidates),
                        (expected_tried, expected_unusable),
                        "seed {seed}: rejection counts wrong for target {boundary}"
                    );
                    rejected += 1;
                }
                Err(other) => panic!("seed {seed}: unexpected rejection {other:?}"),
            }
        }
    }

    assert!(checked > 0, "the oracle never ran");
    assert!(
        snapped > 0,
        "the oracle never observed a snap, so the snap-outward walk is unverified here"
    );
    let _ = rejected;
}
