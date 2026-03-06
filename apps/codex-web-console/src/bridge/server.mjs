import http from "node:http";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import { randomUUID } from "node:crypto";

const PORT = Number(process.env.CODEX_WEB_BRIDGE_PORT ?? "4123");

function autoCodexBin() {
  // apps/codex-web-console -> ../../codex-rs/target/debug/codex(.exe)
  const p = path.resolve(
    process.cwd(),
    "..",
    "..",
    "codex-rs",
    "target",
    "debug",
    process.platform === "win32" ? "codex.exe" : "codex",
  );
  if (fs.existsSync(p)) return p;
  return null;
}

const CODEX_BIN = process.env.CODEX_BIN ?? autoCodexBin() ?? "codex";

function jsonLine(obj) {
  return `${JSON.stringify(obj)}\n`;
}

const proc = spawn(CODEX_BIN, ["app-server"], {
  stdio: "pipe",
  env: process.env,
  windowsHide: true,
});

const pending = new Map(); // id -> {resolve,reject,method}
const ring = [];
const ringMax = 5000;

/** @type {Map<string, Set<http.ServerResponse>>} */
const sseByThread = new Map();

function pushRing(msg) {
  ring.push({ ts: Date.now(), msg });
  if (ring.length > ringMax) ring.splice(0, ring.length - ringMax);
}

function broadcastToThread(threadId, event, data) {
  const set = sseByThread.get(threadId);
  if (!set || set.size === 0) return;
  const payload =
    (event ? `event: ${event}\n` : "") + `data: ${JSON.stringify(data)}\n\n`;
  for (const res of set) {
    try {
      res.write(payload);
    } catch {
      // ignore
    }
  }
}

function extractThreadId(msg) {
  if (!msg || typeof msg !== "object") return null;
  const params = msg.params;
  if (!params || typeof params !== "object") return null;
  const p = params;
  if (typeof p.threadId === "string") return p.threadId;
  if (typeof p.thread_id === "string") return p.thread_id;
  if (p.thread && typeof p.thread === "object" && typeof p.thread.id === "string") return p.thread.id;
  if (typeof p.conversationId === "string") return p.conversationId;
  return null;
}

const stdout = createInterface({ input: proc.stdout });
stdout.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let msg;
  try {
    msg = JSON.parse(trimmed);
  } catch {
    pushRing(trimmed);
    return;
  }

  pushRing(msg);

  // Response
  if (msg && typeof msg === "object" && "id" in msg && ("result" in msg || "error" in msg)) {
    const id = String(msg.id);
    const p = pending.get(id);
    if (p) {
      pending.delete(id);
      if ("error" in msg) p.reject(msg.error);
      else p.resolve(msg.result);
    }
    // Also broadcast responses if they have a threadId inside result (rare); otherwise ignore.
    return;
  }

  // Notifications and server-initiated requests: broadcast to thread stream if we can locate thread id.
  const tid = extractThreadId(msg);
  if (tid) broadcastToThread(tid, "event", msg);
});

proc.stderr.on("data", (chunk) => {
  const s = chunk.toString("utf-8");
  pushRing(`[stderr] ${s}`);
});

proc.on("error", (err) => pushRing(`[proc error] ${String(err)}`));

function rpc(method, params) {
  const id = randomUUID();
  const p = new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }));
  proc.stdin.write(jsonLine({ id, method, params }));
  return p;
}

async function initializeOnce() {
  try {
    await rpc("initialize", {
      clientInfo: { name: "codex_web_bridge", title: "Codex Web Bridge", version: "0.0.1" },
      capabilities: null,
    });
    // required follow-up notification
    proc.stdin.write(jsonLine({ method: "initialized", params: {} }));
  } catch (e) {
    // app-server may be already initialized; ignore.
    pushRing({ initError: String(e) });
  }
}

await initializeOnce();

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "127.0.0.1"}`);

    if (req.method === "GET" && url.pathname === "/health") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, pid: process.pid, codexBin: CODEX_BIN }));
      return;
    }

    if (req.method === "GET" && url.pathname === "/debug") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, pid: process.pid, ring: ring.slice(-500) }));
      return;
    }

    if (req.method === "POST" && url.pathname === "/rpc") {
      let body = "";
      req.setEncoding("utf-8");
      req.on("data", (c) => (body += c));
      req.on("end", async () => {
        let parsed;
        try {
          parsed = JSON.parse(body || "{}");
        } catch {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: "invalid json" }));
          return;
        }
        const method = parsed?.method;
        const params = parsed?.params ?? {};
        if (typeof method !== "string") {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: "missing method" }));
          return;
        }
        try {
          const result = await rpc(method, params);
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: true, result }));
        } catch (err) {
          res.writeHead(500, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: err }));
        }
      });
      return;
    }

    if (req.method === "POST" && url.pathname === "/respond") {
      let body = "";
      req.setEncoding("utf-8");
      req.on("data", (c) => (body += c));
      req.on("end", async () => {
        let parsed;
        try {
          parsed = JSON.parse(body || "{}");
        } catch {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: "invalid json" }));
          return;
        }
        const id = parsed?.id;
        const result = parsed?.result ?? {};
        if (typeof id !== "string") {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: "missing id" }));
          return;
        }
        proc.stdin.write(jsonLine({ id, result }));
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true }));
      });
      return;
    }

    if (req.method === "GET" && url.pathname === "/events") {
      const threadId = url.searchParams.get("threadId");
      if (!threadId) {
        res.writeHead(400, { "content-type": "text/plain" });
        res.end("missing threadId");
        return;
      }

      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache, no-transform",
        connection: "keep-alive",
        "access-control-allow-origin": "*",
      });
      res.write(`event: hello\ndata: ${JSON.stringify({ threadId })}\n\n`);

      let set = sseByThread.get(threadId);
      if (!set) {
        set = new Set();
        sseByThread.set(threadId, set);
      }
      set.add(res);

      const ping = setInterval(() => {
        try {
          res.write(`event: ping\ndata: ${JSON.stringify({ ts: Date.now() })}\n\n`);
        } catch {
          // ignore
        }
      }, 15_000);

      req.on("close", () => {
        clearInterval(ping);
        set.delete(res);
      });
      return;
    }

    res.writeHead(404, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false, error: "not found" }));
  } catch (err) {
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false, error: String(err) }));
  }
});

server.listen(PORT, "127.0.0.1", () => {
  pushRing({ bridge: "listening", port: PORT });
});

