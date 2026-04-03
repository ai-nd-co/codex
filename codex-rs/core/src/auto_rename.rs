use crate::Prompt;
use crate::client_common::ResponseEvent;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::compact::content_items_to_text;
use codex_git_utils::get_git_repo_root;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::InputModality;
use futures::prelude::*;

const AUTO_RENAME_PROMPT: &str = include_str!("../templates/auto_rename/prompt.md");
const MAX_HISTORY_ITEMS: usize = 20;

pub(crate) async fn generate_name(
    sess: &Session,
    turn_context: &TurnContext,
) -> CodexResult<String> {
    let history = sess.clone_history().await;
    let mut items = history.for_prompt(&[InputModality::Text]);
    if items.is_empty() {
        return Err(CodexErr::InvalidRequest(
            "No conversation history to generate a name from.".into(),
        ));
    }

    if items.len() > MAX_HISTORY_ITEMS {
        items = items.split_off(items.len() - MAX_HISTORY_ITEMS);
    }

    let repo_name = get_git_repo_root(&turn_context.cwd)
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| {
            turn_context
                .cwd
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default()
        });

    let prompt = Prompt {
        input: items,
        base_instructions: BaseInstructions {
            text: AUTO_RENAME_PROMPT.replace("{repo}", &repo_name),
        },
        ..Default::default()
    };

    let turn_metadata_header = turn_context.turn_metadata_state.current_header_value();
    let mut client_session = sess.services.model_client.new_session();
    let mut stream = client_session
        .stream(
            &prompt,
            &turn_context.model_info,
            &turn_context.session_telemetry,
            turn_context.reasoning_effort,
            turn_context.reasoning_summary,
            turn_context.config.service_tier,
            turn_metadata_header.as_deref(),
        )
        .await?;

    let mut name_text = String::new();
    loop {
        let Some(event) = stream.next().await else {
            return Err(CodexErr::Stream(
                "stream closed before response.completed".into(),
                None,
            ));
        };
        match event {
            Ok(ResponseEvent::OutputTextDelta(delta)) => name_text.push_str(&delta),
            Ok(ResponseEvent::OutputItemDone(item)) => {
                if let ResponseItem::Message { content, .. } = &item
                    && let Some(text) = content_items_to_text(content)
                    && name_text.is_empty()
                {
                    name_text = text;
                }
            }
            Ok(ResponseEvent::Completed { .. }) => break,
            Ok(_) => continue,
            Err(error) => return Err(error),
        }
    }

    Ok(name_text.trim().to_string())
}
