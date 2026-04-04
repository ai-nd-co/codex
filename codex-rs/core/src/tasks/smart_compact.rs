use std::sync::Arc;

use super::SessionTask;
use super::SessionTaskContext;
use crate::codex::TurnContext;
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Default)]
pub(crate) struct SmartCompactTask;

impl SessionTask for SmartCompactTask {
    fn kind(&self) -> TaskKind {
        TaskKind::SmartCompact
    }

    fn span_name(&self) -> &'static str {
        "session_task.smart_compact"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        _cancellation_token: CancellationToken,
    ) -> Option<String> {
        let session = session.clone_session();
        session
            .services
            .session_telemetry
            .counter("codex.task.smart_compact", /*inc*/ 1, &[]);
        let _ = crate::compact::run_smart_compact_task(session, ctx, input).await;
        None
    }
}
