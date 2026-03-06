"use client";

import React from "react";
import { MarkdownMessage } from "./markdown";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";

export type ChatEvent =
  | { kind: "user"; ts: string; text: string }
  | {
      kind: "assistant";
      ts: string;
      text: string;
      itemId?: string;
      phase?: "delta" | "final";
    }
  | {
      kind: "tool";
      ts: string;
      name: string;
      itemId?: string;
      phase?: "started" | "completed" | "delta";
      input?: unknown;
      output?: unknown;
    }
  | {
      kind: "approval";
      ts: string;
      requestId: string;
      method: string;
      params: unknown;
      status?: "pending" | "accepted" | "declined" | "sent";
    }
  | {
      kind: "exec";
      ts: string;
      itemId?: string;
      tool?: string;
      cmd: string;
      cwd?: string;
      args?: unknown;
      status?: "begin" | "end";
      exitCode?: number | null;
      output?: string;
      durationMs?: number | null;
      processId?: string | null;
    }
  | { kind: "other"; ts: string; type: string; payload: unknown };

function timeLabel(ts: string) {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleTimeString();
}

function Bubble(props: {
  side: "left" | "right";
  title?: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  const { side, title, subtitle, children } = props;

  const align = side === "right" ? "justify-end" : "justify-start";
  const tone =
    side === "right"
      ? "bg-emerald-500/10 border-emerald-500/20"
      : "bg-zinc-500/10 border-zinc-500/20";

  return (
    <div className={`flex ${align}`}>
      <div className={`w-full max-w-[78ch] rounded-2xl border px-4 py-3 ${tone}`}>
        {title ? (
          <div className="flex items-baseline justify-between gap-4">
            <div className="text-xs font-medium text-foreground/80">{title}</div>
            {subtitle ? (
              <div className="text-[11px] text-muted-foreground">{subtitle}</div>
            ) : null}
          </div>
        ) : null}
        <div className={title ? "mt-2" : ""}>{children}</div>
      </div>
    </div>
  );
}

function JsonBlock(props: { value: unknown }) {
  return (
    <pre className="mt-2 overflow-auto rounded-lg border bg-black/30 p-3 text-xs leading-5">
      {JSON.stringify(props.value, null, 2)}
    </pre>
  );
}

function isDiffLike(s: string) {
  const t = s.trim();
  if (!t) return false;
  return (
    t.startsWith("*** Begin Patch") ||
    t.startsWith("diff --git") ||
    t.includes("\n@@") ||
    t.includes("\n+++ ") ||
    t.includes("\n--- ")
  );
}

function ValueBlock(props: { value: unknown }) {
  const { value } = props;

  if (typeof value === "string") {
    if (!value.trim()) {
      return (
        <div className="mt-2 text-xs text-muted-foreground">(empty)</div>
      );
    }
    if (isDiffLike(value)) return <CodeBlock code={value} language="diff" />;
    if (value.includes("\n")) return <CodeBlock code={value} language="text" />;
    return (
      <pre className="mt-2 overflow-auto rounded-lg border bg-black/30 p-3 text-xs leading-5">
        {value}
      </pre>
    );
  }

  return <JsonBlock value={value} />;
}

function CodeBlock(props: { code: string; language?: string }) {
  const { code, language } = props;
  return (
    <div className="mt-2 overflow-hidden rounded-lg border bg-[#0d1117]">
      <SyntaxHighlighter
        language={language}
        style={vscDarkPlus}
        customStyle={{
          margin: 0,
          padding: "12px",
          background: "transparent",
          fontSize: "12px",
          lineHeight: "1.55",
        }}
      >
        {code.replace(/\n$/, "")}
      </SyntaxHighlighter>
    </div>
  );
}

export function ChatTimeline(props: {
  events: ChatEvent[];
  showRaw?: boolean;
  onApprovalDecision?: (req: { requestId: string; decision: "accept" | "decline" }) => void;
}) {
  const { events, showRaw, onApprovalDecision } = props;

  return (
    <div className="space-y-3">
      {events.map((e, idx) => {
        if (e.kind === "user") {
          return (
            <Bubble
              key={idx}
              side="right"
              title="You"
              subtitle={timeLabel(e.ts)}
            >
              <MarkdownMessage text={e.text} />
            </Bubble>
          );
        }
        if (e.kind === "assistant") {
          return (
            <Bubble
              key={idx}
              side="left"
              title="Codex"
              subtitle={timeLabel(e.ts)}
            >
              <MarkdownMessage text={e.text} />
            </Bubble>
          );
        }
        if (e.kind === "tool") {
          if (e.name === "turn/diff/updated") {
            const diff =
              e.output &&
              typeof e.output === "object" &&
              e.output !== null &&
              "diff" in (e.output as Record<string, unknown>)
                ? String((e.output as Record<string, unknown>).diff ?? "")
                : "";
            return (
              <Bubble
                key={idx}
                side="left"
                title="Diff updated"
                subtitle={timeLabel(e.ts)}
              >
                {diff ? <CodeBlock code={diff} language="diff" /> : <JsonBlock value={e.output} />}
              </Bubble>
            );
          }

          if (e.name === "mcpToolCall") {
            // expected shape from SSE mapping:
            // input: { server, tool, arguments }
            // output: { status, result, error, durationMs }
            const input =
              e.input && typeof e.input === "object" && e.input !== null
                ? (e.input as Record<string, unknown>)
                : null;
            const output =
              e.output && typeof e.output === "object" && e.output !== null
                ? (e.output as Record<string, unknown>)
                : null;

            const server = input && typeof input.server === "string" ? input.server : null;
            const tool = input && typeof input.tool === "string" ? input.tool : null;

            const title = server && tool ? `MCP: ${server}.${tool}` : "MCP tool";

            return (
              <Bubble
                key={idx}
                side="left"
                title={title}
                subtitle={timeLabel(e.ts)}
              >
                {output && typeof output.status === "string" ? (
                  <div className="text-[11px] text-muted-foreground">
                    status: {output.status}
                    {typeof output.durationMs === "number"
                      ? ` • ${output.durationMs}ms`
                      : ""}
                  </div>
                ) : null}

                {input && "arguments" in input ? (
                  <>
                    <div className="mt-3 text-xs font-medium text-foreground/80">
                      Arguments
                    </div>
                    <ValueBlock value={input.arguments} />
                  </>
                ) : null}

                {output && "result" in output && output.result !== null ? (
                  <>
                    <div className="mt-3 text-xs font-medium text-foreground/80">
                      Result
                    </div>
                    <ValueBlock value={output.result} />
                  </>
                ) : null}

                {output && "error" in output && output.error !== null ? (
                  <>
                    <div className="mt-3 text-xs font-medium text-foreground/80">
                      Error
                    </div>
                    <ValueBlock value={output.error} />
                  </>
                ) : null}
              </Bubble>
            );
          }

          if (e.name === "fileChange") {
            const changes =
              e.output &&
              typeof e.output === "object" &&
              e.output !== null &&
              "changes" in (e.output as Record<string, unknown>)
                ? (e.output as Record<string, unknown>).changes
                : null;
            if (Array.isArray(changes) && changes.length) {
              return (
                <Bubble
                  key={idx}
                  side="left"
                  title={`File change${e.phase === "completed" ? " (completed)" : ""}`}
                  subtitle={timeLabel(e.ts)}
                >
                  <div className="space-y-3">
                    {changes.map((c, i) => {
                      const rec =
                        c && typeof c === "object"
                          ? (c as Record<string, unknown>)
                          : {};
                      const filePath =
                        typeof rec.path === "string" ? rec.path : "(unknown)";
                      const kind = typeof rec.kind === "string" ? rec.kind : "";
                      const diff =
                        typeof rec.diff === "string" ? rec.diff : "";

                      return (
                      <div key={i} className="rounded-xl border bg-black/10 p-3">
                        <div className="flex items-baseline justify-between gap-3">
                          <div className="min-w-0 truncate text-xs font-medium">
                            {filePath}
                          </div>
                          <div className="text-[11px] text-muted-foreground">
                            {kind}
                          </div>
                        </div>
                        {diff.trim() ? (
                          <CodeBlock code={diff} language="diff" />
                        ) : (
                          <div className="mt-2 text-xs text-muted-foreground">
                            (no diff)
                          </div>
                        )}
                      </div>
                      );
                    })}
                  </div>
                </Bubble>
              );
            }
          }

          return (
            <Bubble
              key={idx}
              side="left"
              title={`Tool: ${e.name}`}
              subtitle={timeLabel(e.ts)}
            >
              {e.input !== undefined ? (
                <>
                  <div className="text-xs font-medium text-foreground/80">
                    Input
                  </div>
                  <ValueBlock value={e.input} />
                </>
              ) : null}
              {e.output !== undefined ? (
                <>
                  <div className="mt-3 text-xs font-medium text-foreground/80">
                    Output
                  </div>
                  <ValueBlock value={e.output} />
                </>
              ) : null}
              {showRaw ? (
                <div className="mt-3 text-[11px] text-muted-foreground">
                  (tool events rendered as JSON for now)
                </div>
              ) : null}
            </Bubble>
          );
        }
        if (e.kind === "approval") {
          const isPending = (e.status ?? "pending") === "pending";
          const params =
            e.params && typeof e.params === "object" && e.params !== null
              ? (e.params as Record<string, unknown>)
              : null;

          const reason =
            params && typeof params.reason === "string" ? params.reason : null;

          return (
            <Bubble
              key={idx}
              side="left"
              title="Approval required"
              subtitle={timeLabel(e.ts)}
            >
              <div className="text-xs text-muted-foreground">
                {e.method}
              </div>

              {reason ? (
                <div className="mt-2 text-sm leading-6">{reason}</div>
              ) : null}

              {e.method === "item/commandExecution/requestApproval" && params ? (
                <>
                  {typeof params.cwd === "string" ? (
                    <div className="mt-2 text-[11px] text-muted-foreground">
                      cwd: {params.cwd}
                    </div>
                  ) : null}
                  {typeof params.command === "string" ? (
                    <CodeBlock code={params.command} language="bash" />
                  ) : (
                    <ValueBlock value={e.params} />
                  )}
                  {"commandActions" in params ? (
                    <>
                      <div className="mt-3 text-xs font-medium text-foreground/80">
                        Parsed actions
                      </div>
                      <ValueBlock value={params.commandActions} />
                    </>
                  ) : null}
                  {"proposedExecpolicyAmendment" in params ? (
                    <>
                      <div className="mt-3 text-xs font-medium text-foreground/80">
                        Proposed policy
                      </div>
                      <ValueBlock value={params.proposedExecpolicyAmendment} />
                    </>
                  ) : null}
                </>
              ) : e.method === "item/fileChange/requestApproval" && params ? (
                <>
                  {"grantRoot" in params ? (
                    <>
                      <div className="mt-2 text-[11px] text-muted-foreground">
                        grantRoot: {String(params.grantRoot ?? "")}
                      </div>
                    </>
                  ) : null}
                  <ValueBlock value={e.params} />
                </>
              ) : (
                <ValueBlock value={e.params} />
              )}

              <div className="mt-3 flex items-center gap-2">
                <button
                  className="h-9 rounded-md border px-3 text-sm font-medium hover:bg-accent/40 disabled:opacity-50"
                  disabled={!isPending}
                  onClick={() =>
                    onApprovalDecision?.({
                      requestId: e.requestId,
                      decision: "accept",
                    })
                  }
                >
                  Accept
                </button>
                <button
                  className="h-9 rounded-md border px-3 text-sm font-medium hover:bg-accent/40 disabled:opacity-50"
                  disabled={!isPending}
                  onClick={() =>
                    onApprovalDecision?.({
                      requestId: e.requestId,
                      decision: "decline",
                    })
                  }
                >
                  Decline
                </button>
                <div className="text-xs text-muted-foreground">
                  {e.status && e.status !== "pending" ? e.status : null}
                </div>
              </div>
            </Bubble>
          );
        }
        if (e.kind === "exec") {
          return (
            <Bubble
              key={idx}
              side="left"
              title={`${e.tool ? `${e.tool}: ` : ""}Exec${e.status ? ` (${e.status})` : ""}`}
              subtitle={timeLabel(e.ts)}
            >
              {e.cwd ? (
                <div className="text-[11px] text-muted-foreground">
                  cwd: {e.cwd}
                  {e.processId ? ` • pid: ${e.processId}` : ""}
                </div>
              ) : null}

              <CodeBlock code={e.cmd} language="bash" />

              {e.args !== undefined ? (
                <>
                  <div className="mt-3 text-xs font-medium text-foreground/80">
                    Args
                  </div>
                  <ValueBlock value={e.args} />
                </>
              ) : null}

              {e.output ? (
                <pre className="mt-2 overflow-auto rounded-lg border bg-black/30 p-3 text-xs leading-5">
                  {e.output}
                </pre>
              ) : null}
              {e.status === "end" ? (
                <div className="mt-2 text-[11px] text-muted-foreground">
                  exit: {e.exitCode ?? "?"}
                  {typeof e.durationMs === "number" ? ` • ${e.durationMs}ms` : ""}
                </div>
              ) : null}
            </Bubble>
          );
        }
        return (
          <Bubble
            key={idx}
            side="left"
            title={e.type}
            subtitle={timeLabel(e.ts)}
          >
            {showRaw ? <JsonBlock value={e.payload} /> : <div className="text-xs text-muted-foreground">Hidden (toggle raw to view)</div>}
          </Bubble>
        );
      })}
    </div>
  );
}
