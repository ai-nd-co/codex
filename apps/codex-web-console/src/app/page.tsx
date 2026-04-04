"use client";

import React from "react";
import { ChatTimeline, type ChatEvent } from "@/components/chat/timeline";
import { markApprovalRequestSent, takeNextQueuedDraft } from "@/lib/chat-state";

type Project = { id: string; path: string; createdAt: number };
type Thread = {
  id: string;
  preview?: string;
  createdAt: number;
  updatedAt?: number;
  cwd?: string;
};

function safeStringify(x: unknown) {
  try {
    return JSON.stringify(x);
  } catch {
    return String(x);
  }
}

function parseSseEventToChatEvent(msg: unknown): ChatEvent | null {
  if (!msg || typeof msg !== "object") return null;
  const m = msg as Record<string, unknown>;
  const method = m.method;
  const params = m.params;
  const ts = new Date().toISOString();

  const approvalRequestMethods = new Set([
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "applyPatchApproval",
    "execCommandApproval",
  ]);

  // Server-initiated requests (approvals, dynamic tools, request_user_input, etc.)
  if (
    typeof method === "string" &&
    "id" in m &&
    (typeof m.id === "string" || typeof m.id === "number") &&
    "params" in m
  ) {
    const requestId = String(m.id);
    if (approvalRequestMethods.has(method)) {
      return {
        kind: "approval",
        ts,
        requestId,
        method,
        params,
        status: "pending",
      };
    }

    return {
      kind: "tool",
      ts,
      name: `request:${method}`,
      phase: "started",
      output: { requestId, method, params },
    };
  }

  // Prefer stable v2 notifications.
  if (method === "item/started" || method === "item/completed") {
    const item =
      params && typeof params === "object" && "item" in (params as Record<string, unknown>)
        ? (params as Record<string, unknown>).item
        : null;
    if (!item || typeof item !== "object") return null;
    const t =
      "type" in (item as Record<string, unknown>)
        ? (item as Record<string, unknown>).type
        : null;
    if (t === "userMessage") {
      // Avoid duplicating the same user message for item/started + item/completed.
      if (method === "item/completed") return null;

      const content =
        "content" in (item as Record<string, unknown>) &&
        Array.isArray((item as Record<string, unknown>).content)
          ? ((item as Record<string, unknown>).content as Array<unknown>)
          : [];
      const text = content
        .map((c) =>
          c && typeof c === "object" && "text" in (c as Record<string, unknown>)
            ? (c as Record<string, unknown>).text
            : null,
        )
        .filter((x): x is string => typeof x === "string")
        .join("");
      return text ? { kind: "user", ts, text } : null;
    }
    if (t === "agentMessage") {
      // completed carries final `text`
      const text =
        "text" in (item as Record<string, unknown>) &&
        typeof (item as Record<string, unknown>).text === "string"
          ? ((item as Record<string, unknown>).text as string)
          : "";
      const itemId =
        "id" in (item as Record<string, unknown>)
          ? String((item as Record<string, unknown>).id)
          : undefined;
      return text
        ? {
            kind: "assistant",
            ts,
            text,
            itemId,
            phase: method === "item/completed" ? "final" : undefined,
          }
        : null;
    }
    if (t === "commandExecution") {
      const cmd =
        "command" in (item as Record<string, unknown>)
          ? String((item as Record<string, unknown>).command ?? "")
          : "";
      const cwd =
        "cwd" in (item as Record<string, unknown>) &&
        typeof (item as Record<string, unknown>).cwd === "string"
          ? ((item as Record<string, unknown>).cwd as string)
          : undefined;
      const aggregatedOutput =
        "aggregatedOutput" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).aggregatedOutput
          : undefined;
      const output = typeof aggregatedOutput === "string" ? aggregatedOutput : undefined;
      const itemId =
        "id" in (item as Record<string, unknown>)
          ? String((item as Record<string, unknown>).id)
          : undefined;
      const processId =
        "processId" in (item as Record<string, unknown>) &&
        (typeof (item as Record<string, unknown>).processId === "string" ||
          (item as Record<string, unknown>).processId === null)
          ? ((item as Record<string, unknown>).processId as string | null)
          : null;
      const durationMs =
        "durationMs" in (item as Record<string, unknown>) &&
        (typeof (item as Record<string, unknown>).durationMs === "number" ||
          (item as Record<string, unknown>).durationMs === null)
          ? ((item as Record<string, unknown>).durationMs as number | null)
          : null;

      const commandActions =
        "commandActions" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).commandActions
          : undefined;
      return {
        kind: "exec",
        ts,
        itemId,
        tool: "commandExecution",
        cmd,
        cwd,
        args: { actions: commandActions },
        processId,
        durationMs,
        status: method === "item/started" ? "begin" : "end",
        output,
        exitCode:
          "exitCode" in (item as Record<string, unknown>) &&
          typeof (item as Record<string, unknown>).exitCode === "number"
            ? ((item as Record<string, unknown>).exitCode as number)
            : null,
      };
    }
    if (t === "fileChange") {
      const itemId =
        "id" in (item as Record<string, unknown>)
          ? String((item as Record<string, unknown>).id)
          : undefined;
      return {
        kind: "tool",
        ts,
        name: "fileChange",
        itemId,
        phase: method === "item/started" ? "started" : "completed",
        output: item,
      };
    }
    if (t === "mcpToolCall") {
      const itemId =
        "id" in (item as Record<string, unknown>)
          ? String((item as Record<string, unknown>).id)
          : undefined;

      const server =
        "server" in (item as Record<string, unknown>) &&
        typeof (item as Record<string, unknown>).server === "string"
          ? ((item as Record<string, unknown>).server as string)
          : "";
      const tool =
        "tool" in (item as Record<string, unknown>) &&
        typeof (item as Record<string, unknown>).tool === "string"
          ? ((item as Record<string, unknown>).tool as string)
          : "";
      const arguments_ =
        "arguments" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).arguments
          : undefined;
      const status =
        "status" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).status
          : undefined;
      const result =
        "result" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).result
          : undefined;
      const error =
        "error" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).error
          : undefined;
      const durationMs =
        "durationMs" in (item as Record<string, unknown>)
          ? (item as Record<string, unknown>).durationMs
          : undefined;
      return {
        kind: "tool",
        ts,
        name: "mcpToolCall",
        itemId,
        phase: method === "item/started" ? "started" : "completed",
        input: { server, tool, arguments: arguments_ },
        output: { status, result, error, durationMs },
      };
    }
    const itemId =
      item && typeof item === "object" && "id" in (item as Record<string, unknown>)
        ? String((item as Record<string, unknown>).id)
        : undefined;
    return {
      kind: "tool",
      ts,
      name: String(t ?? "item"),
      itemId,
      phase: method === "item/started" ? "started" : "completed",
      output: item,
    };
  }

  if (method === "item/agentMessage/delta") {
    const delta =
      params && typeof params === "object" && "delta" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).delta ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return delta
      ? { kind: "assistant", ts, text: delta, itemId, phase: "delta" }
      : null;
  }

  if (method === "item/commandExecution/outputDelta") {
    const delta =
      params && typeof params === "object" && "delta" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).delta ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return delta
      ? { kind: "exec", ts, itemId, cmd: "", output: delta }
      : null;
  }

  if (method === "item/commandExecution/terminalInteraction") {
    const stdin =
      params && typeof params === "object" && "stdin" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).stdin ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return stdin
      ? { kind: "tool", ts, name: "terminalInteraction", itemId, phase: "delta", output: stdin }
      : null;
  }

  if (method === "item/fileChange/outputDelta") {
    const delta =
      params && typeof params === "object" && "delta" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).delta ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return delta
      ? { kind: "tool", ts, name: "fileChange/outputDelta", itemId, phase: "delta", output: delta }
      : null;
  }

  if (method === "item/mcpToolCall/progress") {
    const message =
      params && typeof params === "object" && "message" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).message ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return message
      ? { kind: "tool", ts, name: "mcpToolCall/progress", itemId, phase: "delta", output: message }
      : null;
  }

  if (method === "item/plan/delta") {
    const delta =
      params && typeof params === "object" && "delta" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).delta ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return delta
      ? { kind: "tool", ts, name: "plan/delta", itemId, phase: "delta", output: delta }
      : null;
  }

  if (method === "item/reasoning/summaryTextDelta" || method === "item/reasoning/textDelta") {
    const delta =
      params && typeof params === "object" && "delta" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).delta ?? "")
        : "";
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    const name = method === "item/reasoning/summaryTextDelta" ? "reasoning/summary" : "reasoning/text";
    return delta
      ? { kind: "tool", ts, name, itemId, phase: "delta", output: delta }
      : null;
  }

  if (method === "item/reasoning/summaryPartAdded") {
    const itemId =
      params && typeof params === "object" && "itemId" in (params as Record<string, unknown>)
        ? String((params as Record<string, unknown>).itemId ?? "")
        : undefined;
    return { kind: "tool", ts, name: "reasoning/summaryPartAdded", itemId, phase: "delta", output: params };
  }

  if (method === "turn/plan/updated" || method === "turn/diff/updated") {
    return {
      kind: "tool",
      ts,
      name: String(method),
      phase: "delta",
      output: params,
    };
  }

  return null;
}

function applyChatEvent(prev: ChatEvent[], chatEvent: ChatEvent): ChatEvent[] {
  if (chatEvent.kind === "approval") {
    // Avoid duplicates if the server retries.
    const exists = prev.some(
      (e) => e.kind === "approval" && e.requestId === chatEvent.requestId,
    );
    if (exists) return prev;
    return [...prev, chatEvent];
  }

  if (chatEvent.kind === "exec" && chatEvent.itemId) {
    // outputDelta: append to last matching exec bubble
    if (chatEvent.cmd === "" && chatEvent.output) {
      for (let i = prev.length - 1; i >= 0; i--) {
        const e = prev[i];
        if (e.kind !== "exec") continue;
        if (e.itemId !== chatEvent.itemId) continue;
        const merged: ChatEvent = {
          ...e,
          output: String(e.output ?? "") + chatEvent.output,
        };
        return [...prev.slice(0, i), merged, ...prev.slice(i + 1)];
      }
      // No existing exec bubble yet; ignore delta until started arrives.
      return prev;
    }

    // started/completed: update existing exec bubble if present
    for (let i = prev.length - 1; i >= 0; i--) {
      const e = prev[i];
      if (e.kind !== "exec") continue;
      if (e.itemId !== chatEvent.itemId) continue;
      const mergedArgs =
        e.args &&
        chatEvent.args &&
        typeof e.args === "object" &&
        e.args !== null &&
        typeof chatEvent.args === "object" &&
        chatEvent.args !== null
          ? {
              ...(e.args as Record<string, unknown>),
              ...(chatEvent.args as Record<string, unknown>),
            }
          : chatEvent.args ?? e.args;
      const merged: ChatEvent = {
        ...e,
        tool: chatEvent.tool ?? e.tool,
        cmd: chatEvent.cmd || e.cmd,
        cwd: chatEvent.cwd ?? e.cwd,
        args: mergedArgs,
        processId: chatEvent.processId ?? e.processId,
        status: chatEvent.status ?? e.status,
        exitCode: chatEvent.exitCode ?? e.exitCode,
        output: chatEvent.output ?? e.output,
        durationMs: chatEvent.durationMs ?? e.durationMs,
      };
      return [...prev.slice(0, i), merged, ...prev.slice(i + 1)];
    }
    return [...prev, chatEvent];
  }

  if (chatEvent.kind === "assistant" && chatEvent.itemId) {
    for (let i = prev.length - 1; i >= 0; i--) {
      const e = prev[i];
      if (e.kind !== "assistant") continue;
      if (e.itemId !== chatEvent.itemId) continue;
      const mergedText =
        chatEvent.phase === "delta" ? e.text + chatEvent.text : chatEvent.text;
      const merged: ChatEvent = {
        ...e,
        text: mergedText,
        phase: chatEvent.phase ?? e.phase,
      };
      return [...prev.slice(0, i), merged, ...prev.slice(i + 1)];
    }
    return [...prev, chatEvent];
  }

  if (chatEvent.kind === "tool" && chatEvent.itemId) {
    for (let i = prev.length - 1; i >= 0; i--) {
      const e = prev[i];
      if (e.kind !== "tool") continue;
      if (e.itemId !== chatEvent.itemId) continue;
      if (e.name !== chatEvent.name) continue;
      const mergedOutput =
        e.phase === "delta" &&
        chatEvent.phase === "delta" &&
        typeof e.output === "string" &&
        typeof chatEvent.output === "string"
          ? e.output + chatEvent.output
          : chatEvent.output ?? e.output;
      const merged: ChatEvent = {
        ...e,
        ...chatEvent,
        input: chatEvent.input ?? e.input,
        output: mergedOutput,
      };
      return [...prev.slice(0, i), merged, ...prev.slice(i + 1)];
    }
    return [...prev, chatEvent];
  }

  return [...prev, chatEvent];
}

export default function Home() {
  const [status, setStatus] = React.useState<
    "unknown" | "connecting" | "ok" | "error"
  >("unknown");

  const [projects, setProjects] = React.useState<Project[]>([]);
  const [threads, setThreads] = React.useState<Thread[]>([]);
  const [selectedProjectId, setSelectedProjectId] = React.useState<string | null>(
    null,
  );
  const [selectedThreadId, setSelectedThreadId] = React.useState<string | null>(
    null,
  );

  const [projectPath, setProjectPath] = React.useState("C:\\projects\\logitex");
  const [events, setEvents] = React.useState<ChatEvent[]>([]);
  const [raw, setRaw] = React.useState<string[]>([]);
  const [showRaw, setShowRaw] = React.useState(false);
  const [isWorking, setIsWorking] = React.useState(false);
  const [draft, setDraft] = React.useState("");
  const [queued, setQueued] = React.useState<string[]>([]);
  const [pendingApprovals, setPendingApprovals] = React.useState<Record<string, true>>({});
  const [isCreatingThread, setIsCreatingThread] = React.useState(false);
  const [lastTokens, setLastTokens] = React.useState<{
    totalTokens: number;
    inputTokens: number;
    cachedInputTokens: number;
    outputTokens: number;
    reasoningOutputTokens: number;
  } | null>(null);

  const refreshProjects = React.useCallback(async () => {
    const res = await fetch("/api/projects", { cache: "no-store" });
    const json = await res.json();
    if (json?.ok) setProjects(json.projects);
  }, []);

  const refreshThreads = React.useCallback(async () => {
    if (!selectedProjectId) return;
    const res = await fetch(`/api/projects/${selectedProjectId}/threads`, {
      cache: "no-store",
    });
    const json = await res.json();
    if (json?.ok) setThreads(json.threads);
  }, [selectedProjectId]);

  const loadThreadHistory = React.useCallback(async (threadId: string) => {
    const res = await fetch(`/api/threads/${threadId}/history`, { cache: "no-store" });
    const json = await res.json();
    if (json?.ok && Array.isArray(json.events)) {
      setEvents(json.events);
    } else {
      setEvents([]);
    }
  }, []);

  const submitText = React.useCallback(async (text: string) => {
    if (!selectedThreadId) return false;
    if (!text.trim()) return false;

    setIsWorking(true);
    const res = await fetch(`/api/threads/${selectedThreadId}/turn`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text }),
    });
    const json = await res.json().catch(() => null);
    const ok = res.ok && json?.ok;
    if (!ok) {
      setRaw((r) => [`[turn/start error] ${safeStringify(json)}`, ...r].slice(0, 200));
      setIsWorking(false);
    }
    return ok;
  }, [selectedThreadId]);

  const submitDraft = React.useCallback(async () => {
    if (!draft.trim()) return;

    const text = draft;
    setDraft("");
    await submitText(text);
  }, [draft, submitText]);

  const rawToolCallsRef = React.useRef<
    Map<string, { name: string; args: unknown }>
  >(new Map());

  const sendApprovalDecision = React.useCallback(
    async (req: { requestId: string; decision: "accept" | "decline" }) => {
      setPendingApprovals((p) => ({ ...p, [req.requestId]: true }));
      try {
        const res = await fetch("/api/approvals", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ id: req.requestId, decision: req.decision }),
        });
        const json = await res.json().catch(() => null);
        setRaw((r) => [`[approval] ${safeStringify(json)}`, ...r].slice(0, 200));
        if (res.ok && json?.ok) {
          setEvents((prev) => markApprovalRequestSent(prev, req.requestId));
        }
      } catch (error) {
        setRaw((r) => [`[approval error] ${String(error)}`, ...r].slice(0, 200));
      } finally {
        setPendingApprovals((p) => {
          const next = { ...p };
          delete next[req.requestId];
          return next;
        });
      }
    },
    [],
  );

  // Codex status polling (also used as a "working indicator" sanity check).
  React.useEffect(() => {
    let cancelled = false;

    async function run() {
      setStatus("connecting");
      try {
        const res = await fetch("/api/codex/status", { cache: "no-store" });
        const json = await res.json();
        if (cancelled) return;
        setStatus(json?.ok ? "ok" : "error");
      } catch {
        if (cancelled) return;
        setStatus("error");
      }
    }

    void run();
    const t = setInterval(run, 8_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, []);

  // Load projects on boot.
  React.useEffect(() => {
    void refreshProjects();
  }, [refreshProjects]);

  // Load threads when project changes.
  React.useEffect(() => {
    setThreads([]);
    setSelectedThreadId(null);
    if (!selectedProjectId) return;
    void refreshThreads();
  }, [refreshThreads, selectedProjectId]);

  React.useEffect(() => {
    const { nextDraft, remainingQueued } = takeNextQueuedDraft({
      draft,
      isWorking,
      queued,
      selectedThreadId,
    });
    if (!nextDraft) {
      return;
    }
    setQueued(remainingQueued);
    void submitText(nextDraft);
  }, [draft, isWorking, queued, selectedThreadId, submitText]);

  // Live event stream for selected thread.
  React.useEffect(() => {
    if (!selectedThreadId) return;
    setRaw([]);
    setIsWorking(false);
    void loadThreadHistory(selectedThreadId);
    const es = new EventSource(`/api/threads/${selectedThreadId}/events`);

    es.addEventListener("event", (evt) => {
      const data = (evt as MessageEvent).data as string;
      setRaw((t) => [data, ...t].slice(0, 500));

      // Minimal "working" heuristic
      try {
        const msg = JSON.parse(data) as unknown;
        const method =
          msg && typeof msg === "object" && "method" in (msg as Record<string, unknown>)
            ? (msg as Record<string, unknown>).method
            : null;
        if (method === "turn/started") setIsWorking(true);
        if (method === "turn/completed") {
          setIsWorking(false);
          // refresh full history snapshot after completion (so we can show the "first message" preview, etc.)
          void loadThreadHistory(selectedThreadId);
        }
        if (method === "thread/tokenUsage/updated") {
          const p =
            msg && typeof msg === "object" && "params" in (msg as Record<string, unknown>)
              ? ((msg as Record<string, unknown>).params as unknown)
              : null;
          const tokenUsage =
            p && typeof p === "object" && p !== null && "tokenUsage" in (p as Record<string, unknown>)
              ? (p as Record<string, unknown>).tokenUsage
              : null;
          const last =
            tokenUsage &&
            typeof tokenUsage === "object" &&
            tokenUsage !== null &&
            "last" in (tokenUsage as Record<string, unknown>)
              ? (tokenUsage as Record<string, unknown>).last
              : null;
          if (last && typeof last === "object" && last !== null) {
            const rec = last as Record<string, unknown>;
            const next = {
              totalTokens: Number(rec.totalTokens ?? 0),
              inputTokens: Number(rec.inputTokens ?? 0),
              cachedInputTokens: Number(rec.cachedInputTokens ?? 0),
              outputTokens: Number(rec.outputTokens ?? 0),
              reasoningOutputTokens: Number(rec.reasoningOutputTokens ?? 0),
            };
            setLastTokens(next);
          }
        }

        // Capture raw ResponseItems (tool calls + outputs) when enabled.
        if (method === "rawResponseItem/completed") {
          const params =
            msg && typeof msg === "object" && "params" in (msg as Record<string, unknown>)
              ? ((msg as Record<string, unknown>).params as unknown)
              : null;
          const item =
            params &&
            typeof params === "object" &&
            params !== null &&
            "item" in (params as Record<string, unknown>)
              ? (params as Record<string, unknown>).item
              : null;

          if (item && typeof item === "object" && item !== null) {
            const it = item as Record<string, unknown>;
            const itemType = it.type;

            if (itemType === "function_call") {
              const name = typeof it.name === "string" ? it.name : "";
              const callId = typeof it.call_id === "string" ? it.call_id : "";
              const argsText = typeof it.arguments === "string" ? it.arguments : "";

              let args: unknown = argsText;
              if (argsText) {
                try {
                  args = JSON.parse(argsText);
                } catch {
                  // keep as string
                }
              }

              if (callId && name) rawToolCallsRef.current.set(callId, { name, args });

              // For non-exec tools, render tool-call args explicitly.
              if (callId && name && name !== "exec_command") {
                const toolCallEvent: ChatEvent = {
                  kind: "tool",
                  ts: new Date().toISOString(),
                  name: `toolCall:${name}`,
                  itemId: callId,
                  phase: "started",
                  input: args,
                };
                setEvents((prev) => applyChatEvent(prev, toolCallEvent).slice(-2000));
              }
            }

            if (itemType === "function_call_output") {
              const callId = typeof it.call_id === "string" ? it.call_id : "";
              const out = "output" in it ? it.output : undefined;
              const call = callId ? rawToolCallsRef.current.get(callId) : null;
              const name = call?.name ?? "function_call";

              // For non-exec tools, attach output to the existing toolCall bubble.
              if (callId && name && name !== "exec_command") {
                const toolOutEvent: ChatEvent = {
                  kind: "tool",
                  ts: new Date().toISOString(),
                  name: `toolCall:${name}`,
                  itemId: callId,
                  phase: "completed",
                  output: out,
                };
                setEvents((prev) => applyChatEvent(prev, toolOutEvent).slice(-2000));
              }
            }
          }

          return;
        }

        let chatEvent = parseSseEventToChatEvent(msg);
        if (chatEvent) {
          // Attach raw tool-call args (when available) to matching commandExecution items.
          if (chatEvent.kind === "exec" && chatEvent.itemId) {
            const raw = rawToolCallsRef.current.get(chatEvent.itemId);
            if (raw) {
              const baseArgs =
                chatEvent.args && typeof chatEvent.args === "object" && chatEvent.args !== null
                  ? (chatEvent.args as Record<string, unknown>)
                  : {};
              chatEvent = {
                ...chatEvent,
                tool: raw.name || chatEvent.tool,
                args: raw.args !== undefined ? { ...baseArgs, tool: raw.args } : chatEvent.args,
              };
            }
	          }
	
	          const evt = chatEvent;
	          setEvents((prev) => applyChatEvent(prev, evt).slice(-2000));
	        }
	      } catch {
	        // ignore
	      }
    });

    es.addEventListener("error", () => {
      setRaw((t) => [`[sse error] ${new Date().toISOString()}`, ...t]);
    });

    return () => es.close();
  }, [loadThreadHistory, selectedThreadId, setRaw]);

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-4">
          <div className="flex items-baseline gap-3">
            <div className="text-lg font-semibold tracking-tight">Codex</div>
            <div className="text-xs text-muted-foreground">Agent Console</div>
          </div>
          <div className="flex items-center gap-3 text-xs">
            <div>
              Status:{" "}
              <span
                className={
                  status === "ok"
                    ? "text-emerald-600"
                    : status === "connecting"
                      ? "text-amber-600"
                      : "text-red-600"
                }
              >
                {status}
              </span>
            </div>
            <div className="text-muted-foreground">•</div>
            <div>
              Model:{" "}
              <span className={isWorking ? "text-amber-600" : "text-emerald-600"}>
                {isWorking ? "working" : "idle"}
              </span>
            </div>
            {Object.keys(pendingApprovals).length ? (
              <>
                <div className="text-muted-foreground">•</div>
                <div className="text-amber-600">
                  approvals: {Object.keys(pendingApprovals).length}
                </div>
              </>
            ) : null}
            {lastTokens ? (
              <>
                <div className="text-muted-foreground">•</div>
                <div className="text-muted-foreground">
                  tokens(last): in {lastTokens.inputTokens} (cached{" "}
                  {lastTokens.cachedInputTokens}) / out {lastTokens.outputTokens} /
                  total {lastTokens.totalTokens}
                </div>
              </>
            ) : null}
            <div className="text-muted-foreground">•</div>
            <button
              className="rounded-md border px-2 py-1 text-[11px] hover:bg-accent/40"
              onClick={() => setShowRaw((v) => !v)}
              title="Toggle raw event log"
            >
              raw: {showRaw ? "on" : "off"}
            </button>
          </div>
        </div>
      </header>

      <main className="mx-auto grid max-w-6xl grid-cols-12 gap-6 px-6 py-6">
        <aside className="col-span-4 rounded-2xl border bg-card/40 backdrop-blur">
          <div className="border-b px-4 py-3">
            <div className="text-sm font-medium">Project</div>
            <div className="mt-2 flex gap-2">
              <input
                className="h-9 w-full rounded-md border bg-background/40 px-3 text-sm"
                placeholder="C:\\projects\\logitex"
                value={projectPath}
                onChange={(e) => setProjectPath(e.target.value)}
              />
              <button
                className="h-9 rounded-md border px-3 text-sm font-medium hover:bg-accent/40"
                onClick={async () => {
                  const res = await fetch("/api/projects", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ path: projectPath }),
                  });
                  const json = await res.json();
                  if (json?.ok) setSelectedProjectId(json.project.id);
                  await refreshProjects();
                }}
              >
                Add
              </button>
            </div>
          </div>

          <div className="p-3">
            <div className="space-y-1">
              {projects.map((p) => (
                <button
                  key={p.id}
                  className={
                    "w-full rounded-xl border px-3 py-2 text-left text-sm " +
                    (selectedProjectId === p.id
                      ? "border-foreground/30 bg-accent/40"
                      : "border-border/60 hover:bg-accent/30")
                  }
                  onClick={() => setSelectedProjectId(p.id)}
                >
                  <div className="truncate font-medium">{p.path}</div>
                  <div className="text-xs text-muted-foreground">
                    {new Date(p.createdAt).toLocaleDateString()}
                  </div>
                </button>
              ))}
              {!projects.length ? (
                <div className="px-2 py-3 text-xs text-muted-foreground">
                  Add a project path to begin.
                </div>
              ) : null}
            </div>
          </div>

          <div className="border-t px-4 py-3">
            <div className="flex items-center justify-between">
              <div className="text-sm font-medium">Conversations</div>
              <button
                className="h-8 rounded-md border px-2 text-xs font-medium hover:bg-accent/40 disabled:opacity-50"
                disabled={!selectedProjectId || isCreatingThread}
                onClick={async () => {
                  if (!selectedProjectId) return;
                  setIsCreatingThread(true);
                  try {
                    const res = await fetch(
                      `/api/projects/${selectedProjectId}/threads`,
                      {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({}),
                      },
                    );
                    const json = await res.json().catch(() => null);
                    if (!json?.ok) {
                      setRaw((r) => [`[thread/start error] ${safeStringify(json)}`, ...r].slice(0, 200));
                      return;
                    }
                    setSelectedThreadId(json.thread.id);
                    void loadThreadHistory(json.thread.id);
                    await refreshThreads();
                  } catch (err) {
                    setRaw((r) => [`[thread/start exception] ${String(err)}`, ...r].slice(0, 200));
                  } finally {
                    setIsCreatingThread(false);
                  }
                }}
              >
                {isCreatingThread ? "New…" : "New"}
              </button>
            </div>
            <div className="mt-2 text-[11px] text-muted-foreground">
              Named by first message preview.
            </div>
          </div>

          <div className="p-3 pt-0">
            <div className="space-y-1">
              {threads.map((t) => (
                <button
                  key={t.id}
                  className={
                    "w-full rounded-xl border px-3 py-2 text-left text-sm " +
                    (selectedThreadId === t.id
                      ? "border-foreground/30 bg-accent/40"
                      : "border-border/60 hover:bg-accent/30")
                  }
                  onClick={() => {
                    setSelectedThreadId(t.id);
                    void loadThreadHistory(t.id);
                  }}
                >
                  <div className="truncate font-medium">
                    {t.preview?.trim()
                      ? t.preview.trim()
                      : "(new conversation)"}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {new Date((t.updatedAt ?? t.createdAt) * 1000).toLocaleString()}
                  </div>
                </button>
              ))}
              {!selectedProjectId ? (
                <div className="px-2 py-3 text-xs text-muted-foreground">
                  Select a project.
                </div>
              ) : !threads.length ? (
                <div className="px-2 py-3 text-xs text-muted-foreground">
                  No conversations yet.
                </div>
              ) : null}
            </div>
          </div>
        </aside>

        <section className="col-span-8 flex min-h-[75vh] flex-col rounded-2xl border bg-card/40 backdrop-blur">
          <div className="border-b px-4 py-3">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">
                  {selectedThreadId
                    ? threads.find((t) => t.id === selectedThreadId)?.preview?.trim() ||
                      "Conversation"
                    : "Select a conversation"}
                </div>
                <div className="text-xs text-muted-foreground truncate">
                  {selectedThreadId
                    ? `thread: ${selectedThreadId}`
                    : "Choose a project and a conversation to start."}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <div
                  className={
                    "h-2 w-2 rounded-full " +
                    (isWorking ? "bg-amber-500" : "bg-emerald-500")
                  }
                  title={isWorking ? "working" : "idle"}
                />
                <div className="text-xs text-muted-foreground">
                  {isWorking ? "working" : "idle"}
                </div>
              </div>
            </div>
          </div>

          <div className="flex-1 overflow-auto p-4">
            {selectedThreadId ? (
              events.length ? (
                <ChatTimeline
                  events={events}
                  showRaw={showRaw}
                  onApprovalDecision={sendApprovalDecision}
                />
              ) : (
                <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                  New conversation. Send the first message.
                </div>
              )
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                Select a project, then a conversation.
              </div>
            )}
          </div>

          {showRaw ? (
            <div className="border-t p-3">
              <div className="text-xs font-medium text-foreground/80">Raw</div>
              <pre className="mt-2 max-h-48 overflow-auto rounded-lg border bg-black/30 p-3 text-[11px] leading-5">
                {raw.slice(0, 200).join("\n")}
              </pre>
            </div>
          ) : null}

          <div className="border-t p-4">
            <textarea
              className="min-h-24 w-full resize-none rounded-xl border bg-background/40 p-3 text-sm leading-6 outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (!selectedThreadId) return;
                if (isWorking && e.key === "Tab") {
                  e.preventDefault();
                  if (draft.trim()) {
                    setQueued((q) => [...q, draft]);
                    setDraft("");
                  }
                  return;
                }
                if (!isWorking && e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void submitDraft();
                }
              }}
              placeholder={
                selectedThreadId ? "Message Codex…" : "Select a conversation…"
              }
              disabled={!selectedThreadId}
            />
            <div className="mt-2 flex items-center justify-between">
              <div className="text-xs text-muted-foreground">
                Tab queues while working. Shift+Enter for newline. queued:{" "}
                {queued.length}
              </div>
              <button
                className="h-9 rounded-md border px-3 text-sm font-medium hover:bg-accent/40 disabled:opacity-50"
                disabled={!selectedThreadId || !draft.trim()}
                onClick={() => void submitDraft()}
              >
                Send
              </button>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
