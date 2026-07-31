use super::*;
use anyhow::Result;
use codex_protocol::protocol::TurnAbortReason;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn client_response_payload_returns_jsonrpc_parts_and_client_response() -> Result<()> {
    let (request_id, result, payload) =
        ClientResponsePayload::ThreadArchive(v2::ThreadArchiveResponse {})
            .into_jsonrpc_parts_and_payload(RequestId::Integer(7))?;

    assert_eq!(request_id, RequestId::Integer(7));
    assert_eq!(result, json!({}));

    let Some(ClientResponse::ThreadArchive {
        request_id,
        response: _,
    }) = payload.and_then(|payload| payload.into_client_response(RequestId::Integer(7)))
    else {
        panic!("expected thread/archive client response");
    };
    assert_eq!(request_id, RequestId::Integer(7));
    Ok(())
}

/// Locks the `thread/smartCompact/start` wire contract: the method name, the camelCase params
/// field, and that it is experimental-gated so it stays out of the vendored stable schema fixtures
/// while `smart_compact` is `Stage::UnderDevelopment`.
#[test]
fn thread_smart_compact_start_wire_contract() -> Result<()> {
    let request = ClientRequest::ThreadSmartCompactStart {
        request_id: RequestId::Integer(11),
        params: v2::ThreadSmartCompactStartParams {
            thread_id: "thread-1".to_string(),
        },
    };

    assert_eq!(
        serde_json::to_value(&request)?,
        json!({
            "method": "thread/smartCompact/start",
            "id": 11,
            "params": { "threadId": "thread-1" },
        })
    );
    assert_eq!(
        crate::experimental_api::ExperimentalApi::experimental_reason(&request),
        Some("thread/smartCompact/start")
    );
    // Same serialization scope as `thread/compact/start`: both replace the thread's history.
    assert_eq!(
        request.serialization_scope(),
        Some(ClientRequestSerializationScope::Thread {
            thread_id: "thread-1".to_string(),
        })
    );

    let (request_id, result, _) =
        ClientResponsePayload::ThreadSmartCompactStart(v2::ThreadSmartCompactStartResponse {})
            .into_jsonrpc_parts_and_payload(RequestId::Integer(11))?;
    assert_eq!(request_id, RequestId::Integer(11));
    assert_eq!(result, json!({}));

    Ok(())
}

#[test]
fn interrupt_conversation_payload_stays_jsonrpc_only() -> Result<()> {
    let (request_id, result, payload) =
        ClientResponsePayload::InterruptConversation(v1::InterruptConversationResponse {
            abort_reason: TurnAbortReason::Interrupted,
        })
        .into_jsonrpc_parts_and_payload(RequestId::Integer(8))?;

    assert_eq!(request_id, RequestId::Integer(8));
    assert_eq!(
        result,
        json!({
            "abortReason": "interrupted",
        })
    );
    assert!(payload.is_none());
    Ok(())
}
