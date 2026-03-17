use crate::Prompt;
use crate::client_common::ResponseEvent;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::compact::content_items_to_text;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::git_info::get_git_repo_root;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::InputModality;
use futures::prelude::*;

const AUTO_RENAME_PROMPT: &str = include_str!("../templates/auto_rename/prompt.md");

/// Maximum number of conversation items to include in the naming prompt.
const MAX_HISTORY_ITEMS: usize = 20;

/// Run a side-channel LLM call to generate a concise thread name from
/// conversation context. The generated name is NOT recorded into conversation
/// history. Returns the raw name string on success.
pub(crate) async fn generate_name(
    sess: &Session,
    turn_context: &TurnContext,
) -> CodexResult<String> {
    let history = sess.clone_history().await;
    // Text-only modalities for the naming prompt (no images needed).
    let mut items = history.for_prompt(&[InputModality::Text]);
    if items.is_empty() {
        return Err(CodexErr::InvalidRequest(
            "No conversation history to generate a name from.".into(),
        ));
    }

    // Keep only the last N items to minimize token cost.
    if items.len() > MAX_HISTORY_ITEMS {
        items = items.split_off(items.len() - MAX_HISTORY_ITEMS);
    }

    // Derive repo name from cwd.
    let cwd = &turn_context.cwd;
    let repo_name = get_git_repo_root(cwd)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| {
            cwd.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });

    let prompt_text = AUTO_RENAME_PROMPT.replace("{repo}", &repo_name);

    let prompt = Prompt {
        input: items,
        base_instructions: BaseInstructions { text: prompt_text },
        ..Default::default()
    };

    // Stream the response, collecting text only (not recorded into history).
    let turn_metadata_header = turn_context.turn_metadata_state.current_header_value();
    let mut client_session = sess.services.model_client.new_session();
    let mut stream = client_session
        .stream(
            &prompt,
            &turn_context.model_info,
            &turn_context.otel_manager,
            turn_context.reasoning_effort,
            turn_context.reasoning_summary,
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
            Ok(ResponseEvent::OutputTextDelta(delta)) => {
                name_text.push_str(&delta);
            }
            Ok(ResponseEvent::OutputItemDone(item)) => {
                // Extract text from completed items as fallback.
                if let ResponseItem::Message { content, .. } = &item {
                    if let Some(text) = content_items_to_text(content) {
                        if name_text.is_empty() {
                            name_text = text;
                        }
                    }
                }
            }
            Ok(ResponseEvent::Completed { .. }) => break,
            Ok(_) => continue,
            Err(e) => return Err(e),
        }
    }

    Ok(name_text.trim().to_string())
}
