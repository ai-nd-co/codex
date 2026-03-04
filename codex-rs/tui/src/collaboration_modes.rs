use codex_core::models_manager::manager::ModelsManager;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::config_types::ModeKind;

fn filtered_presets(
    models_manager: &ModelsManager,
    request_user_input_in_default_mode: bool,
) -> Vec<CollaborationModeMask> {
    models_manager
        .list_collaboration_modes(request_user_input_in_default_mode)
        .into_iter()
        .filter(|mask| mask.mode.is_some_and(ModeKind::is_tui_visible))
        .collect()
}

pub(crate) fn presets_for_tui(
    models_manager: &ModelsManager,
    request_user_input_in_default_mode: bool,
) -> Vec<CollaborationModeMask> {
    filtered_presets(models_manager, request_user_input_in_default_mode)
}

pub(crate) fn default_mask(
    models_manager: &ModelsManager,
    request_user_input_in_default_mode: bool,
) -> Option<CollaborationModeMask> {
    let presets = filtered_presets(models_manager, request_user_input_in_default_mode);
    presets
        .iter()
        .find(|mask| mask.mode == Some(ModeKind::Default))
        .cloned()
        .or_else(|| presets.into_iter().next())
}

pub(crate) fn mask_for_kind(
    models_manager: &ModelsManager,
    kind: ModeKind,
    request_user_input_in_default_mode: bool,
) -> Option<CollaborationModeMask> {
    if !kind.is_tui_visible() {
        return None;
    }
    filtered_presets(models_manager, request_user_input_in_default_mode)
        .into_iter()
        .find(|mask| mask.mode == Some(kind))
}

/// Cycle to the next collaboration mode preset in list order.
pub(crate) fn next_mask(
    models_manager: &ModelsManager,
    current: Option<&CollaborationModeMask>,
    request_user_input_in_default_mode: bool,
) -> Option<CollaborationModeMask> {
    let presets = filtered_presets(models_manager, request_user_input_in_default_mode);
    if presets.is_empty() {
        return None;
    }
    let current_kind = current.and_then(|mask| mask.mode);
    let next_index = presets
        .iter()
        .position(|mask| mask.mode == current_kind)
        .map_or(0, |idx| (idx + 1) % presets.len());
    presets.get(next_index).cloned()
}

pub(crate) fn default_mode_mask(
    models_manager: &ModelsManager,
    request_user_input_in_default_mode: bool,
) -> Option<CollaborationModeMask> {
    mask_for_kind(
        models_manager,
        ModeKind::Default,
        request_user_input_in_default_mode,
    )
}

pub(crate) fn plan_mask(
    models_manager: &ModelsManager,
    request_user_input_in_default_mode: bool,
) -> Option<CollaborationModeMask> {
    mask_for_kind(
        models_manager,
        ModeKind::Plan,
        request_user_input_in_default_mode,
    )
}
