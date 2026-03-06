import fs from "node:fs";
import readline from "node:readline";

export type RolloutEvent =
  | {
      ts: string;
      kind: "user";
      text: string;
    }
  | {
      ts: string;
      kind: "assistant";
      text: string;
    }
  | {
      ts: string;
      kind: "tool";
      name: string;
      input?: unknown;
      output?: unknown;
    }
  | {
      ts: string;
      kind: "exec";
      cmd: string;
      cwd?: string;
      args?: unknown;
      tool?: string;
      status?: "begin" | "end";
      exitCode?: number | null;
      output?: string;
      durationMs?: number | null;
      processId?: string | null;
    }
  | {
      ts: string;
      kind: "other";
      type: string;
      payload: unknown;
    };

export async function parseRolloutJsonlFile(filePath: string, limit = 2000) {
  const stream = fs.createReadStream(filePath, { encoding: "utf-8" });
  const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });

  const events: RolloutEvent[] = [];
  const toolCalls = new Map<string, { name: string; input?: unknown; ts: string }>();
  const execByCall = new Map<
    string,
    {
      ts: string;
      cmd?: string;
      cwd?: string;
      args?: unknown;
      exitCode?: number | null;
      output?: string;
      durationMs?: number | null;
      processId?: string | null;
    }
  >();
  const completedExecCalls = new Set<string>();

  for await (const line of rl) {
    if (!line.trim()) continue;
    let obj: unknown;
    try {
      obj = JSON.parse(line);
    } catch {
      continue;
    }

    if (!obj || typeof obj !== "object") continue;
    const rec = obj as Record<string, unknown>;

    const ts = typeof rec.timestamp === "string" ? rec.timestamp : new Date().toISOString();
    const recordType = rec.type;
    const payload = rec.payload;

    // Messages + tool calls are usually under response_item payloads.
    if (recordType === "response_item" && payload && typeof payload === "object") {
      const p = payload as Record<string, unknown>;
      const itemType = p.type;

      if (itemType === "function_call") {
        const name = typeof p.name === "string" ? p.name : "function_call";
        const callId = typeof p.call_id === "string" ? p.call_id : undefined;
        let input: unknown = undefined;
        if (typeof p.arguments === "string") {
          try {
            input = JSON.parse(p.arguments);
          } catch {
            input = p.arguments;
          }
        }
        if (callId) {
          if (name === "exec_command") {
            const prev = execByCall.get(callId) ?? { ts };
            execByCall.set(callId, {
              ...prev,
              ts: prev.ts ?? ts,
              args: input,
            });
          } else {
            toolCalls.set(callId, { name, input, ts });
          }
        } else {
          events.push({ ts, kind: "tool", name, input });
        }
      }

      if (itemType === "function_call_output") {
        const callId = typeof p.call_id === "string" ? p.call_id : undefined;
        if (callId && completedExecCalls.has(callId)) {
          // exec_command outputs may be recorded after exec_command_end; avoid duplicate noise.
          continue;
        }
        let output: unknown = p.output;
        if (typeof output === "string") {
          try {
            output = JSON.parse(output);
          } catch {
            // keep as string
          }
        }
        if (callId && execByCall.has(callId)) {
          const prev = execByCall.get(callId)!;
          // Keep output as a best-effort fallback; exec_command_end usually has the real aggregated output.
          execByCall.set(callId, { ...prev, output: prev.output ?? (typeof output === "string" ? output : JSON.stringify(output)) });
        } else if (callId && toolCalls.has(callId)) {
          const call = toolCalls.get(callId)!;
          events.push({ ts, kind: "tool", name: call.name, input: call.input, output });
          toolCalls.delete(callId);
        } else {
          events.push({ ts, kind: "tool", name: "function_call_output", output });
        }
      }
    }

    // Exec begin/end are under event_msg payloads.
    if (recordType === "event_msg" && payload && typeof payload === "object") {
      const p = payload as Record<string, unknown>;
      const t = p.type;

      if (t === "user_message") {
        const msg = typeof p.message === "string" ? p.message : "";
        if (msg) events.push({ ts, kind: "user", text: msg });
      }

      if (t === "agent_message") {
        const msg = typeof p.message === "string" ? p.message : "";
        if (msg) events.push({ ts, kind: "assistant", text: msg });
      }

      if (t === "agent_reasoning") {
        const text = typeof p.text === "string" ? p.text : "";
        if (text) events.push({ ts, kind: "tool", name: "reasoning", output: text });
      }

      if (t === "exec_command_begin") {
        const callId = typeof p.call_id === "string" ? p.call_id : undefined;
        const cmdArr = Array.isArray(p.command) ? p.command : null;
        const cmd = cmdArr ? cmdArr.join(" ") : "";
        const cwd = typeof p.cwd === "string" ? p.cwd : undefined;
        const processId =
          typeof p.process_id === "string" ? p.process_id : null;
        if (callId) {
          const prev = execByCall.get(callId) ?? { ts };
          execByCall.set(callId, {
            ...prev,
            ts: prev.ts ?? ts,
            cmd: cmd || prev.cmd,
            cwd: cwd ?? prev.cwd,
            processId: processId ?? prev.processId,
          });
        }
      }

      if (t === "exec_command_end") {
        const callId = typeof p.call_id === "string" ? p.call_id : undefined;
        const meta = callId && execByCall.has(callId) ? execByCall.get(callId)! : null;
        const cmd = meta?.cmd ?? "";
        const exitCode =
          typeof p.exit_code === "number" ? p.exit_code : null;
        const aggregated =
          typeof p.aggregated_output === "string" ? p.aggregated_output : undefined;
        const duration =
          p.duration && typeof p.duration === "object"
            ? (p.duration as Record<string, unknown>)
            : null;
        const durationMs =
          duration && typeof duration.secs === "number" && typeof duration.nanos === "number"
            ? Math.round(duration.secs * 1000 + duration.nanos / 1_000_000)
            : null;
        const out = aggregated ?? meta?.output;
        if (meta) {
          events.push({
            ts: meta.ts ?? ts,
            kind: "exec",
            tool: "exec_command",
            cmd,
            cwd: meta.cwd,
            args: meta.args,
            status: "end",
            exitCode,
            output: out,
            durationMs,
            processId: meta.processId ?? null,
          });
        } else {
          events.push({
            ts,
            kind: "exec",
            tool: "exec_command",
            cmd,
            status: "end",
            exitCode,
            output: aggregated,
            durationMs,
          });
        }
        if (callId) execByCall.delete(callId);
        if (callId) completedExecCalls.add(callId);
      }
    }

    if (events.length >= limit) break;
  }

  return events;
}
