import type { ChatEvent } from "@/components/chat/timeline";

export function markApprovalRequestSent(
  events: ChatEvent[],
  requestId: string,
): ChatEvent[] {
  return events.map((event) =>
    event.kind === "approval" && event.requestId === requestId
      ? { ...event, status: "sent" }
      : event,
  );
}

export function takeNextQueuedDraft(opts: {
  draft: string;
  isWorking: boolean;
  queued: string[];
  selectedThreadId: string | null;
}): { nextDraft: string | null; remainingQueued: string[] } {
  const { draft, isWorking, queued, selectedThreadId } = opts;
  if (isWorking || draft.trim() || !selectedThreadId || queued.length === 0) {
    return { nextDraft: null, remainingQueued: queued };
  }

  return {
    nextDraft: queued[0] ?? null,
    remainingQueued: queued.slice(1),
  };
}
