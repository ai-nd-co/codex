use std::sync::Arc;

use super::SessionTask;
use super::SessionTaskContext;
use crate::codex::TurnContext;
use crate::state::TaskKind;
use async_trait::async_trait;
use codex_protocol::user_input::UserInput;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Default)]
pub(crate) struct SmartCompactTask;

#[async_trait]
impl SessionTask for SmartCompactTask {
    fn kind(&self) -> TaskKind {
        TaskKind::SmartCompact
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        _cancellation_token: CancellationToken,
    ) -> Option<String> {
        let session = session.clone_session();
        let _ = session
            .services
            .otel_manager
            .counter("codex.task.smart_compact", 1, &[]);
        crate::compact::run_smart_compact_task(session, ctx, input).await;
        None
    }
}
