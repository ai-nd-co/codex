use super::handlers::user_input_or_turn_inner;
use super::session::Session;
use codex_protocol::protocol::AdditionalContextEntry;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_ACCEPTED_NEXT_TURNS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct NextTurnPayload {
    pub input: Vec<UserInput>,
    pub client_user_message_id: Option<String>,
    pub responsesapi_client_metadata: Option<HashMap<String, String>>,
    pub additional_context: BTreeMap<String, AdditionalContextEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnqueueNextTurnOutcome {
    pub turn_id: String,
    pub duplicate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueNextTurnError {
    EmptyIdempotencyKey,
    EmptyInput,
    IdempotencyConflict,
    CapacityExceeded,
}

#[derive(Clone)]
struct QueuedNextTurn {
    turn_id: String,
    payload: NextTurnPayload,
}

#[derive(Clone)]
struct AcceptedNextTurn {
    turn_id: String,
    payload: NextTurnPayload,
}

#[derive(Default)]
pub(crate) struct NextTurnQueue {
    pending: VecDeque<QueuedNextTurn>,
    accepted_by_idempotency_key: HashMap<String, AcceptedNextTurn>,
    completed_idempotency_keys: VecDeque<String>,
}

impl NextTurnQueue {
    pub(crate) fn enqueue(
        &mut self,
        idempotency_key: String,
        payload: NextTurnPayload,
    ) -> Result<EnqueueNextTurnOutcome, EnqueueNextTurnError> {
        if idempotency_key.is_empty() {
            return Err(EnqueueNextTurnError::EmptyIdempotencyKey);
        }
        if payload.input.is_empty() {
            return Err(EnqueueNextTurnError::EmptyInput);
        }
        if let Some(accepted) = self.accepted_by_idempotency_key.get(&idempotency_key) {
            return if accepted.payload == payload {
                Ok(EnqueueNextTurnOutcome {
                    turn_id: accepted.turn_id.clone(),
                    duplicate: true,
                })
            } else {
                Err(EnqueueNextTurnError::IdempotencyConflict)
            };
        }
        while self.accepted_by_idempotency_key.len() >= MAX_ACCEPTED_NEXT_TURNS {
            let Some(completed_key) = self.completed_idempotency_keys.pop_front() else {
                return Err(EnqueueNextTurnError::CapacityExceeded);
            };
            self.accepted_by_idempotency_key.remove(&completed_key);
        }
        if self.pending.len() >= MAX_ACCEPTED_NEXT_TURNS {
            return Err(EnqueueNextTurnError::CapacityExceeded);
        }

        let turn_id = uuid::Uuid::now_v7().to_string();
        self.accepted_by_idempotency_key.insert(
            idempotency_key,
            AcceptedNextTurn {
                turn_id: turn_id.clone(),
                payload: payload.clone(),
            },
        );
        self.pending.push_back(QueuedNextTurn {
            turn_id: turn_id.clone(),
            payload,
        });
        Ok(EnqueueNextTurnOutcome {
            turn_id,
            duplicate: false,
        })
    }

    fn pop_front(&mut self) -> Option<QueuedNextTurn> {
        self.pending.pop_front()
    }

    fn push_front(&mut self, queued: QueuedNextTurn) {
        self.pending.push_front(queued);
    }

    pub(crate) fn mark_turn_finished(&mut self, turn_id: &str) {
        let Some((idempotency_key, _)) = self
            .accepted_by_idempotency_key
            .iter()
            .find(|(_, accepted)| accepted.turn_id == turn_id)
        else {
            return;
        };
        if !self
            .completed_idempotency_keys
            .iter()
            .any(|completed| completed == idempotency_key)
        {
            self.completed_idempotency_keys
                .push_back(idempotency_key.clone());
        }
    }
}

impl Session {
    pub(crate) async fn enqueue_next_turn(
        self: &Arc<Self>,
        idempotency_key: String,
        payload: NextTurnPayload,
    ) -> Result<EnqueueNextTurnOutcome, EnqueueNextTurnError> {
        let outcome = self
            .next_turn_queue
            .lock()
            .await
            .enqueue(idempotency_key, payload)?;
        self.maybe_start_queued_next_turn().await;
        Ok(outcome)
    }

    pub(crate) async fn maybe_start_queued_next_turn(self: &Arc<Self>) {
        let _turn_start_guard = self.turn_start_lock.lock().await;
        if self.active_turn.lock().await.is_some() {
            return;
        }
        let Some(queued) = self.next_turn_queue.lock().await.pop_front() else {
            return;
        };
        let payload = queued.payload.clone();
        let op = Op::UserInput {
            items: payload.input,
            final_output_json_schema: None,
            responsesapi_client_metadata: payload.responsesapi_client_metadata,
            additional_context: payload.additional_context,
            thread_settings: ThreadSettingsOverrides::default(),
        };
        let started = user_input_or_turn_inner(
            self,
            queued.turn_id.clone(),
            op,
            payload.client_user_message_id,
        )
        .await;
        if !started {
            self.next_turn_queue.lock().await.push_front(queued);
        }
    }

    pub(crate) async fn mark_queued_turn_finished(&self, turn_id: &str) {
        self.next_turn_queue
            .lock()
            .await
            .mark_turn_finished(turn_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn payload(text: &str) -> NextTurnPayload {
        NextTurnPayload {
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            client_user_message_id: None,
            responsesapi_client_metadata: None,
            additional_context: BTreeMap::new(),
        }
    }

    #[test]
    fn queue_preserves_fifo_and_deduplicates_identical_payloads() {
        let mut queue = NextTurnQueue::default();
        let first = queue
            .enqueue("first".to_string(), payload("one"))
            .expect("first accepted");
        let duplicate = queue
            .enqueue("first".to_string(), payload("one"))
            .expect("duplicate accepted");
        let second = queue
            .enqueue("second".to_string(), payload("two"))
            .expect("second accepted");

        assert_eq!(duplicate.turn_id, first.turn_id);
        assert!(duplicate.duplicate);
        assert_eq!(queue.pending.len(), 2);
        assert_eq!(queue.pop_front().expect("first").turn_id, first.turn_id);
        assert_eq!(queue.pop_front().expect("second").turn_id, second.turn_id);
    }

    #[test]
    fn queue_rejects_conflicting_identity_reuse() {
        let mut queue = NextTurnQueue::default();
        queue
            .enqueue("same".to_string(), payload("one"))
            .expect("first accepted");
        assert_eq!(
            queue.enqueue("same".to_string(), payload("different")),
            Err(EnqueueNextTurnError::IdempotencyConflict)
        );
        assert_eq!(queue.pending.len(), 1);
    }

    #[test]
    fn queue_is_bounded_without_evicting_idempotency_records() {
        let mut queue = NextTurnQueue::default();
        for index in 0..MAX_ACCEPTED_NEXT_TURNS {
            queue
                .enqueue(index.to_string(), payload("same"))
                .expect("within capacity");
        }
        assert_eq!(
            queue.enqueue("overflow".to_string(), payload("same")),
            Err(EnqueueNextTurnError::CapacityExceeded)
        );
        let duplicate = queue
            .enqueue("0".to_string(), payload("same"))
            .expect("existing identity remains retryable");
        assert!(duplicate.duplicate);
    }

    #[test]
    fn completed_identity_is_evicted_to_recover_capacity() {
        let mut queue = NextTurnQueue::default();
        let first = queue
            .enqueue("0".to_string(), payload("first"))
            .expect("first accepted");
        assert_eq!(
            queue.pop_front().expect("first started").turn_id,
            first.turn_id
        );
        queue.mark_turn_finished(&first.turn_id);
        for index in 1..MAX_ACCEPTED_NEXT_TURNS {
            queue
                .enqueue(index.to_string(), payload("same"))
                .expect("within capacity");
        }

        queue
            .enqueue("replacement".to_string(), payload("replacement"))
            .expect("completed identity frees capacity");
        assert!(!queue.accepted_by_idempotency_key.contains_key("0"));
    }

    #[test]
    fn failed_start_can_restore_the_same_queue_head() {
        let mut queue = NextTurnQueue::default();
        let accepted = queue
            .enqueue("retry".to_string(), payload("retry me"))
            .expect("accepted");
        let queued = queue.pop_front().expect("queued entry");
        queue.push_front(queued);

        assert_eq!(
            queue.pop_front().expect("restored entry").turn_id,
            accepted.turn_id
        );
        assert_eq!(
            queue
                .enqueue("retry".to_string(), payload("retry me"))
                .expect("retry accepted")
                .turn_id,
            accepted.turn_id
        );
    }
}
