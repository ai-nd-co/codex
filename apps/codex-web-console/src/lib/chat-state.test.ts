import { describe, expect, it } from "vitest";
import { markApprovalRequestSent, takeNextQueuedDraft } from "./chat-state";

describe("markApprovalRequestSent", () => {
  it("marks only the matching approval as sent", () => {
    const events = [
      {
        kind: "approval" as const,
        ts: "2026-04-05T00:00:00.000Z",
        requestId: "a",
        method: "execCommandApproval",
        params: {},
        status: "pending" as const,
      },
      {
        kind: "approval" as const,
        ts: "2026-04-05T00:00:01.000Z",
        requestId: "b",
        method: "execCommandApproval",
        params: {},
        status: "pending" as const,
      },
    ];

    expect(markApprovalRequestSent(events, "b")).toEqual([
      events[0],
      { ...events[1], status: "sent" },
    ]);
  });
});

describe("takeNextQueuedDraft", () => {
  it("returns the next queued draft when idle with an active thread", () => {
    expect(
      takeNextQueuedDraft({
        draft: "",
        isWorking: false,
        queued: ["first", "second"],
        selectedThreadId: "thread-1",
      }),
    ).toEqual({
      nextDraft: "first",
      remainingQueued: ["second"],
    });
  });

  it("does not dequeue while still working or while the textarea has content", () => {
    expect(
      takeNextQueuedDraft({
        draft: "current text",
        isWorking: false,
        queued: ["first"],
        selectedThreadId: "thread-1",
      }),
    ).toEqual({
      nextDraft: null,
      remainingQueued: ["first"],
    });

    expect(
      takeNextQueuedDraft({
        draft: "",
        isWorking: true,
        queued: ["first"],
        selectedThreadId: "thread-1",
      }),
    ).toEqual({
      nextDraft: null,
      remainingQueued: ["first"],
    });
  });
});
