//! Turn-boundary split selection and call/output closure validation for selective compaction.
//!
//! Selective compaction ("smart compact") replaces the older part of a history with a summary and
//! keeps the newer part verbatim. Choosing where to cut is the correctness-critical step: the two
//! halves are each normalized independently by
//! [`ContextManager::for_prompt`](super::ContextManager::for_prompt), and
//! `normalize::ensure_call_outputs_present` / `normalize::remove_orphan_outputs` react to an
//! unpaired call or output by fabricating an `"aborted"` output or dropping the item, after calling
//! `crate::util::error_or_panic` at five of the seven sites. `error_or_panic` panics under
//! `debug_assertions` and logs an invariant violation otherwise, so a bad cut aborts debug builds
//! and silently mutates model-visible context in release builds.
//!
//! Today's compaction cannot reach any of that, because
//! `compact_remote::should_keep_compacted_history_item` drops every tool and reasoning item, so
//! pairing is satisfied by deletion. Selective compaction is the first caller able to violate the
//! invariant, which is why this module exists before the feature does.
//!
//! Rules encoded here:
//!
//! - **Both halves are validated.** The older half is not discarded: it is the input to the
//!   summarizer, and that request is normalized too. A `FunctionCall` left dangling at the end of
//!   the older half gets an `"aborted"` output fabricated, so the summary then describes a turn
//!   that looks aborted but was not.
//! - **The cut is always derived from a turn boundary**, never from `len / 2`.
//! - **The token target is measured at the turn boundary, not at the snapped index.** Snapping moves an
//!   index earlier by an amount that depends on that turn's pre-turn context, so targeting the snapped
//!   index would let a turn with a large reinjected context hand the target to a *later* boundary and
//!   compact strictly more. For the same reason, a boundary whose snapped cut collapses to `0` is still
//!   targetable and simply has no usable cut. See `boundary_split_pairs`.
//! - **On an unsafe boundary, snap outward** to the next earlier candidate, which compacts *less*.
//!   A conflict is never resolved by dropping or moving an item.
//! - **Reasoning items travel with their turn.** They have no pairing validator anywhere, so
//!   nothing downstream would catch a mis-split; boundary alignment is the only protection.
//! - **Pre-turn context items travel with the turn they describe.** `ContextManager` records
//!   contextual and instruction-carrying items immediately *above* the user message of the turn they
//!   belong to (see `history::trim_pre_turn_context_updates`), so a candidate is snapped back over the
//!   contiguous run of developer messages and contextual user messages above it. That keeps persistent
//!   developer text (a mixed `build_initial_context` bundle, the separate guardian-policy message, the
//!   multi-agent usage hint) in the verbatim half instead of summarizing it away.
//! - **Items that cannot carry turn metadata are inert.** `AdditionalTools`, `CompactionTrigger`
//!   and `Other` have no `internal_chat_message_metadata_passthrough` field
//!   (`codex-protocol/src/models.rs`), so they cannot be attributed to a turn. They never
//!   participate in pairing, are never chosen as a split point, and travel by index with whichever
//!   half they fall in. Nothing is relocated, duplicated or dropped for them.
//!
//! A turn boundary alone does **not** guarantee closure. A `FunctionCall` is written to history the
//! moment the model emits it (`stream_events_utils::handle_output_item_done`) and its output only
//! when the tool future resolves (`session::turn::drain_in_flight`); an interrupt in between leaves
//! a dangling call in history forever, because `tasks::handle_task_abort` only appends a marker and
//! never repairs history. So validation, not boundary alignment, is the gate. Violations are
//! therefore classified: those the split *induced* (the counterpart exists, on the other side) block
//! the candidate, while those already present in the input history are reported and tolerated,
//! because no choice of split can fix them and `for_prompt` already handles them today.

// This module is the S1 half of selective compaction (task 2026-07-30-103). It is deliberately
// additive and has no production call site yet; phase S2 (task 2026-07-30-104) is the consumer.
#![allow(dead_code)]

use codex_protocol::models::ResponseItem;
use std::collections::HashSet;

use super::estimate_item_token_count;
use super::is_user_turn_boundary;
use crate::event_mapping::is_contextual_user_message_content;

/// Fraction of estimated tokens that should land in the older (summarized) half by default.
pub(crate) const DEFAULT_SPLIT_TARGET_FRACTION: f64 = 0.5;

/// Which half of a candidate split an item ended up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SplitSide {
    /// `items[..split_index]`, the part selective compaction replaces with a summary.
    Older,
    /// `items[split_index..]`, the part that stays verbatim.
    Newer,
}

/// A call or output that has no counterpart inside its half.
///
/// Each variant names the exact `normalize.rs` behaviour it would trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClosureViolationKind {
    /// `normalize::ensure_call_outputs_present` fabricates an `"aborted"` `FunctionCallOutput`.
    DanglingFunctionCall,
    /// `normalize::ensure_call_outputs_present` fabricates an empty `ToolSearchOutput`.
    DanglingToolSearchCall,
    /// `normalize::ensure_call_outputs_present` asserts, then fabricates an `"aborted"` output.
    DanglingCustomToolCall,
    /// `normalize::ensure_call_outputs_present` asserts, then fabricates an `"aborted"` output.
    DanglingLocalShellCall,
    /// `normalize::remove_orphan_outputs` asserts, then drops the output.
    OrphanFunctionCallOutput,
    /// `normalize::remove_orphan_outputs` asserts, then drops the output.
    OrphanCustomToolCallOutput,
    /// `normalize::remove_orphan_outputs` asserts, then drops the output.
    OrphanToolSearchOutput,
}

impl ClosureViolationKind {
    /// Whether the matching `normalize.rs` site routes through `crate::util::error_or_panic`, which
    /// panics under `debug_assertions`.
    ///
    /// Both answers are violations that selective compaction must not induce. The silent kinds are
    /// arguably worse: they fabricate an `"aborted"` output that the summarizer then believes.
    pub(crate) fn aborts_debug_builds(self) -> bool {
        match self {
            // Repaired with `info!` and no assertion.
            Self::DanglingFunctionCall | Self::DanglingToolSearchCall => false,
            Self::DanglingCustomToolCall
            | Self::DanglingLocalShellCall
            | Self::OrphanFunctionCallOutput
            | Self::OrphanCustomToolCallOutput
            | Self::OrphanToolSearchOutput => true,
        }
    }
}

/// One unpaired item, located in the *input* history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClosureViolation {
    pub(crate) kind: ClosureViolationKind,
    /// Index of the unpaired item in the input slice, not in the half it landed in.
    pub(crate) index: usize,
    pub(crate) call_id: String,
    pub(crate) side: SplitSide,
}

/// Result of validating one candidate split.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClosureReport {
    /// Violations the split created: the counterpart exists in the input history but landed on the
    /// other side. A candidate with any of these must be rejected.
    pub(crate) induced: Vec<ClosureViolation>,
    /// Violations already present in the input history before any split, annotated with the side
    /// they landed on. No split can fix these and `for_prompt` already handles them today, so they
    /// are reported rather than blamed on the split.
    pub(crate) preexisting: Vec<ClosureViolation>,
}

impl ClosureReport {
    /// True when the split induced no violation. Pre-existing violations do not affect this.
    pub(crate) fn is_closed(&self) -> bool {
        self.induced.is_empty()
    }
}

/// An accepted split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistorySplit {
    /// `items[..split_index]` is summarized; `items[split_index..]` stays verbatim.
    ///
    /// Always in `1..items.len()`, so neither half is empty.
    pub(crate) split_index: usize,
    /// Coarse `estimate_item_token_count` sum over `items[..split_index]`.
    pub(crate) older_token_estimate: i64,
    /// Coarse `estimate_item_token_count` sum over `items[split_index..]`.
    pub(crate) newer_token_estimate: i64,
    /// The **turn boundary** nearest the token-weighted target, before snapping and before any
    /// snap-outward. Always `>= split_index`.
    pub(crate) target_boundary: usize,
    /// How many candidates closure validation rejected before this one was accepted. `0` means the
    /// candidate nearest the target was already safe.
    pub(crate) snapped_outward_by: usize,
    /// Violations that already existed in the input history and survive the split unchanged.
    pub(crate) preexisting_violations: Vec<ClosureViolation>,
}

/// Why no safe split exists. Callers must handle these instead of receiving an unsafe index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SplitRejection {
    /// Nothing to split.
    EmptyHistory,
    /// No `is_user_turn_boundary` item at all, so there is no turn to align to.
    NoTurnBoundary,
    /// Every turn boundary would leave one half empty, so there is nothing to compact separately.
    NoInteriorTurnBoundary { boundaries: usize },
    /// No cut at or before the targeted boundary is both usable and closed, and snapping outward ran
    /// out of boundaries. Snapping *inward* is never attempted: a later cut compacts more than the
    /// caller asked for, which is the direction that loses context.
    NoSafeBoundary {
        /// Usable candidates at or before the target that closure validation rejected.
        candidates_tried: usize,
        /// Boundaries at or before the target whose snapped cut collapsed to index `0`, so it would
        /// have left an empty older half. These were skipped, not validated.
        unusable_candidates: usize,
        /// Violations of the nearest usable candidate, for diagnostics. Empty when every candidate at
        /// or before the target was unusable rather than unsafe.
        nearest_induced: Vec<ClosureViolation>,
    },
}

/// Items that cannot carry `internal_chat_message_metadata_passthrough` and therefore cannot be
/// attributed to a turn.
///
/// `CompactionTrigger` and `Other` additionally fail `history::is_api_message`, so `record_items`
/// never admits them; they are handled here only because `ContextManager::replace` bypasses that
/// filter.
pub(crate) fn is_turn_metadata_less(item: &ResponseItem) -> bool {
    matches!(
        item,
        ResponseItem::AdditionalTools { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::Other
    )
}

/// Indices of items that start a turn, per `history::is_user_turn_boundary`.
pub(crate) fn turn_boundary_indices(items: &[ResponseItem]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| is_user_turn_boundary(item))
        .map(|(index, _)| index)
        .collect()
}

/// `(turn boundary, split index after snapping)` for **every** turn boundary, ascending.
///
/// Two properties of this list are load-bearing, and getting either wrong lets the selector compact
/// *more* than it was asked to:
///
/// 1. **Both fields are kept, and token targeting runs against the boundary.** Snapping moves an index
///    earlier by an amount that depends on how large that turn's pre-turn context is. If the target
///    were chosen among snapped indices, a boundary with a big reinjected context could pull its
///    snapped index far enough from the target that a *later* boundary became the nearest one.
/// 2. **Nothing is filtered here.** A boundary whose snapped index collapses to `0` (everything above
///    it is context, which happens for the first turn of any real session and after compaction inserts
///    initial context at the head) has no usable cut, but it must still be *targetable*. Dropping it
///    before targeting would hand the target to a later boundary, which again compacts more. The
///    conservative answer when the target's own cut is unusable and nothing earlier works is a typed
///    rejection, not a later cut.
///
/// A pair is usable as a split exactly when `split_index > 0`; `split_index <= boundary < items.len()`
/// always holds, so no upper check is needed.
///
/// Both fields are strictly increasing: a boundary is a real user message or an `AgentMessage`, and
/// neither is traversable, so `snapped(b_i) > b_{i-1} >= snapped(b_{i-1})`. No deduplication needed.
fn boundary_split_pairs(items: &[ResponseItem]) -> Vec<(usize, usize)> {
    turn_boundary_indices(items)
        .into_iter()
        .map(|boundary| (boundary, snap_over_pre_turn_context(items, boundary)))
        .collect()
}

/// Usable candidate split indices: turn boundaries snapped back over their own pre-turn context items,
/// keeping only those that leave a non-empty older half, ascending.
pub(crate) fn split_candidates(items: &[ResponseItem]) -> Vec<usize> {
    boundary_split_pairs(items)
        .into_iter()
        .map(|(_, split_index)| split_index)
        .filter(|split_index| *split_index > 0)
        .collect()
}

/// Walk back from a turn boundary over the contiguous run of instruction-carrying and contextual items
/// directly above it, extending `history::trim_pre_turn_context_updates`.
///
/// Two item shapes are traversed:
///
/// 1. **Any `developer`-role message.** Rollback only traverses those with a recognized contextual
///    fragment, but that misses every persistent developer message: the mixed `build_initial_context`
///    bundle's non-contextual half, the separate guardian-policy message, and the multi-agent usage
///    hint. Leaving those in the older half lets summarization replace exact instructions with prose.
/// 2. **A `user`-role message with contextual content**, exactly as rollback detects it.
///
/// **Turn metadata is deliberately not consulted**, and that is a resolved conflict rather than an
/// oversight. Items are stamped with the recording turn's `sub_id`
/// (`session/mod.rs:2813-2816`), so an id-based rule looks more precise, but the two failure modes it
/// trades against are not symmetric:
///
/// - Requiring a *matching* id would strand instructions. Mid-turn compaction stamps freshly built
///   initial context with the compaction turn and then inserts it immediately above the last *retained*
///   real user message, which still carries its own older id or none at all
///   (`compact.rs:553-583` with `session/mod.rs:3483-3486`). Those developer items would be summarized
///   away.
/// - Ignoring ids can only retain *more* verbatim. A previous turn's trailing developer or contextual
///   user items (skill and plugin injection recorded after the input at `session/turn.rs:192-208`, or
///   the interrupt marker at `tasks/mod.rs:97-105`) may be carried into the verbatim half. That
///   reorders nothing, drops nothing, and cannot affect closure.
///
/// For every boundary the content-only walk yields a split index less than or equal to the id-based
/// one. Because token targeting runs against the **boundary** rather than the snapped index (see
/// `boundary_split_pairs`), the same target boundary is chosen either way, so the content-only rule
/// really does summarize a subset. "When ownership is ambiguous, compact less" is the same principle as
/// snapping outward, so it wins.
///
/// The walk cannot cross an earlier boundary: a real user message or an `AgentMessage` is neither a
/// developer message nor contextual user content, so it always stops the walk.
fn snap_over_pre_turn_context(items: &[ResponseItem], boundary: usize) -> usize {
    let mut candidate = boundary;
    while candidate > 0 {
        match &items[candidate - 1] {
            ResponseItem::Message { role, .. } if role == "developer" => {
                candidate -= 1;
            }
            ResponseItem::Message { role, content, .. }
                if role == "user" && is_contextual_user_message_content(content) =>
            {
                candidate -= 1;
            }
            _ => break,
        }
    }
    candidate
}

/// Report which items `split_index` would leave unpaired on each side.
///
/// Usable independently of [`select_compaction_split`]; `split_index` is clamped to `items.len()`.
pub(crate) fn validate_split_closure(items: &[ResponseItem], split_index: usize) -> ClosureReport {
    let split_index = split_index.min(items.len());

    // Splitting can only remove counterparts from a side, never add them, so anything unpaired in
    // the whole history is unpaired in whichever half holds it. That makes the whole-history scan a
    // sound baseline for separating pre-existing violations from split-induced ones.
    let preexisting_keys: HashSet<(ClosureViolationKind, usize)> = collect_unpaired(items, 0)
        .into_iter()
        .map(|unpaired| (unpaired.kind, unpaired.index))
        .collect();

    let (older, newer) = items.split_at(split_index);
    let mut induced = Vec::new();
    let mut preexisting = Vec::new();
    for (unpaired, side) in collect_unpaired(older, 0)
        .into_iter()
        .map(|unpaired| (unpaired, SplitSide::Older))
        .chain(
            collect_unpaired(newer, split_index)
                .into_iter()
                .map(|unpaired| (unpaired, SplitSide::Newer)),
        )
    {
        let violation = unpaired.with_side(side);
        if preexisting_keys.contains(&(violation.kind, violation.index)) {
            preexisting.push(violation);
        } else {
            induced.push(violation);
        }
    }
    induced.sort_unstable_by_key(|violation| violation.index);
    preexisting.sort_unstable_by_key(|violation| violation.index);

    ClosureReport {
        induced,
        preexisting,
    }
}

/// Choose a safe split for selective compaction.
///
/// `target_fraction` is the share of estimated tokens that should land in the older half; it is
/// clamped to `0.0..=1.0` and non-finite values fall back to [`DEFAULT_SPLIT_TARGET_FRACTION`].
///
/// The returned index is guaranteed to induce no closure violation on either side. When no such
/// index exists, a [`SplitRejection`] says why; the caller never receives a silently unsafe index.
pub(crate) fn select_compaction_split(
    items: &[ResponseItem],
    target_fraction: f64,
) -> Result<HistorySplit, SplitRejection> {
    if items.is_empty() {
        return Err(SplitRejection::EmptyHistory);
    }
    let boundaries = turn_boundary_indices(items);
    if boundaries.is_empty() {
        return Err(SplitRejection::NoTurnBoundary);
    }
    let candidates = boundary_split_pairs(items);
    if !candidates.iter().any(|(_, split_index)| *split_index > 0) {
        return Err(SplitRejection::NoInteriorTurnBoundary {
            boundaries: boundaries.len(),
        });
    }

    let prefix_tokens = cumulative_token_estimates(items);
    let total_tokens = prefix_at(&prefix_tokens, items.len());
    let target_tokens = target_token_count(total_tokens, target_fraction);

    // Nearest **turn boundary** to the token-weighted target, deliberately not the nearest snapped
    // index; see `boundary_split_pairs`. Boundaries are ascending and `min_by_key` keeps the first
    // minimum, so a tie resolves to the earlier boundary, which compacts less: the same direction the
    // snap-outward walk takes.
    let (target_position, target_boundary) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, (boundary, _))| {
            prefix_at(&prefix_tokens, *boundary)
                .saturating_sub(target_tokens)
                .saturating_abs()
        })
        .map(|(position, (boundary, _))| (position, *boundary))
        .unwrap_or((0, 0));

    let mut nearest_induced = Vec::new();
    let mut candidates_tried = 0usize;
    let mut unusable_candidates = 0usize;
    // Snap outward, i.e. toward a smaller older half, which compacts less. This can exhaust its
    // candidates; that is a typed rejection, never a fallback to a later cut, which would compact more
    // than the caller asked for.
    for (_, split_index) in candidates
        .get(..=target_position)
        .unwrap_or_default()
        .iter()
        .copied()
        .rev()
    {
        if split_index == 0 {
            // The whole prefix above this boundary is context, so the cut would leave an empty older
            // half. Skip it rather than jumping to a later boundary.
            unusable_candidates += 1;
            continue;
        }
        let report = validate_split_closure(items, split_index);
        if report.is_closed() {
            let older_token_estimate = prefix_at(&prefix_tokens, split_index);
            return Ok(HistorySplit {
                split_index,
                older_token_estimate,
                newer_token_estimate: total_tokens.saturating_sub(older_token_estimate),
                target_boundary,
                snapped_outward_by: candidates_tried,
                preexisting_violations: report.preexisting,
            });
        }
        if candidates_tried == 0 {
            nearest_induced = report.induced;
        }
        candidates_tried += 1;
    }

    Err(SplitRejection::NoSafeBoundary {
        candidates_tried,
        unusable_candidates,
        nearest_induced,
    })
}

/// A call or output with no counterpart in the slice it was scanned in.
struct Unpaired {
    kind: ClosureViolationKind,
    index: usize,
    call_id: String,
}

impl Unpaired {
    fn with_side(self, side: SplitSide) -> ClosureViolation {
        ClosureViolation {
            kind: self.kind,
            index: self.index,
            call_id: self.call_id,
            side,
        }
    }
}

/// Call/output id sets for one slice, mirroring the sets `normalize.rs` builds.
#[derive(Default)]
struct PairIndex<'a> {
    function_call_ids: HashSet<&'a str>,
    tool_search_call_ids: HashSet<&'a str>,
    local_shell_call_ids: HashSet<&'a str>,
    custom_tool_call_ids: HashSet<&'a str>,
    function_output_ids: HashSet<&'a str>,
    tool_search_output_ids: HashSet<&'a str>,
    custom_tool_output_ids: HashSet<&'a str>,
}

impl<'a> PairIndex<'a> {
    fn build(items: &'a [ResponseItem]) -> Self {
        let mut index = Self::default();
        for item in items {
            match item {
                ResponseItem::FunctionCall { call_id, .. } => {
                    index.function_call_ids.insert(call_id.as_str());
                }
                ResponseItem::ToolSearchCall {
                    call_id: Some(call_id),
                    ..
                } => {
                    index.tool_search_call_ids.insert(call_id.as_str());
                }
                ResponseItem::LocalShellCall {
                    call_id: Some(call_id),
                    ..
                } => {
                    index.local_shell_call_ids.insert(call_id.as_str());
                }
                ResponseItem::CustomToolCall { call_id, .. } => {
                    index.custom_tool_call_ids.insert(call_id.as_str());
                }
                ResponseItem::FunctionCallOutput { call_id, .. } => {
                    index.function_output_ids.insert(call_id.as_str());
                }
                ResponseItem::ToolSearchOutput {
                    call_id: Some(call_id),
                    ..
                } => {
                    index.tool_search_output_ids.insert(call_id.as_str());
                }
                ResponseItem::CustomToolCallOutput { call_id, .. } => {
                    index.custom_tool_output_ids.insert(call_id.as_str());
                }
                _ => {}
            }
        }
        index
    }
}

/// Scan one slice for items whose counterpart is missing from that slice.
///
/// `offset` is added to every reported index so violations are located in the input history rather
/// than in the half. The arm order and the membership rules mirror
/// `normalize::ensure_call_outputs_present` and `normalize::remove_orphan_outputs` exactly,
/// including that a `FunctionCallOutput` may be satisfied by either a `FunctionCall` or a
/// `LocalShellCall`, and that a server-executed `ToolSearchOutput` is never treated as an orphan.
fn collect_unpaired(items: &[ResponseItem], offset: usize) -> Vec<Unpaired> {
    let index = PairIndex::build(items);
    let mut unpaired = Vec::new();
    for (position, item) in items.iter().enumerate() {
        let absolute_index = offset.saturating_add(position);
        let mut push = |kind: ClosureViolationKind, call_id: &str| {
            unpaired.push(Unpaired {
                kind,
                index: absolute_index,
                call_id: call_id.to_string(),
            });
        };
        match item {
            ResponseItem::FunctionCall { call_id, .. }
                if !index.function_output_ids.contains(call_id.as_str()) =>
            {
                push(ClosureViolationKind::DanglingFunctionCall, call_id);
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } if !index.tool_search_output_ids.contains(call_id.as_str()) => {
                push(ClosureViolationKind::DanglingToolSearchCall, call_id);
            }
            ResponseItem::CustomToolCall { call_id, .. }
                if !index.custom_tool_output_ids.contains(call_id.as_str()) =>
            {
                push(ClosureViolationKind::DanglingCustomToolCall, call_id);
            }
            ResponseItem::LocalShellCall {
                call_id: Some(call_id),
                ..
            } if !index.function_output_ids.contains(call_id.as_str()) => {
                push(ClosureViolationKind::DanglingLocalShellCall, call_id);
            }
            ResponseItem::FunctionCallOutput { call_id, .. }
                if !index.function_call_ids.contains(call_id.as_str())
                    && !index.local_shell_call_ids.contains(call_id.as_str()) =>
            {
                push(ClosureViolationKind::OrphanFunctionCallOutput, call_id);
            }
            ResponseItem::CustomToolCallOutput { call_id, .. }
                if !index.custom_tool_call_ids.contains(call_id.as_str()) =>
            {
                push(ClosureViolationKind::OrphanCustomToolCallOutput, call_id);
            }
            // `normalize::remove_orphan_outputs` keeps server-executed tool search output
            // unconditionally, before the call-id check, so it can never be an orphan.
            ResponseItem::ToolSearchOutput { execution, .. } if execution == "server" => {}
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                ..
            } if !index.tool_search_call_ids.contains(call_id.as_str()) => {
                push(ClosureViolationKind::OrphanToolSearchOutput, call_id);
            }
            _ => {}
        }
    }
    unpaired
}

/// `prefix[k]` is the estimated token count of `items[..k]`, so `prefix` has `items.len() + 1`
/// entries.
fn cumulative_token_estimates(items: &[ResponseItem]) -> Vec<i64> {
    let mut prefix = Vec::with_capacity(items.len().saturating_add(1));
    let mut running = 0i64;
    prefix.push(running);
    for item in items {
        running = running.saturating_add(estimate_item_token_count(item));
        prefix.push(running);
    }
    prefix
}

fn prefix_at(prefix: &[i64], index: usize) -> i64 {
    prefix.get(index).copied().unwrap_or(0)
}

fn target_token_count(total_tokens: i64, target_fraction: f64) -> i64 {
    let fraction = if target_fraction.is_finite() {
        target_fraction.clamp(0.0, 1.0)
    } else {
        DEFAULT_SPLIT_TARGET_FRACTION
    };
    // `as i64` saturates on overflow and on NaN yields 0; `fraction` is already finite and clamped.
    (total_tokens.max(0) as f64 * fraction).round() as i64
}

// `pub(crate)` so phase S2 (`compact_smart`) reuses this module's fixtures and, above all, its
// `for_prompt_roundtrip` harness instead of writing a second copy of the debug-normalization proof.
#[cfg(test)]
#[path = "split_tests.rs"]
pub(crate) mod tests;
