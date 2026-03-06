import { spawn } from "node:child_process";
import path from "node:path";

const portIdx = process.argv.indexOf("--port");
const PORT = portIdx >= 0 ? process.argv[portIdx + 1] : process.env.PORT ?? "3010";
const BRIDGE_PORT = process.env.CODEX_WEB_BRIDGE_PORT ?? "4123";

function run(cmd, args, extraEnv = {}) {
  const p = spawn(cmd, args, {
    stdio: "inherit",
    env: { ...process.env, ...extraEnv },
    windowsHide: true,
  });
  return p;
}

const bridgePath = path.resolve("src", "bridge", "server.mjs");

const bridge = run(process.execPath, [bridgePath], {
  CODEX_WEB_BRIDGE_PORT: BRIDGE_PORT,
});

const nextBin = path.resolve("node_modules", "next", "dist", "bin", "next");
const next = run(
  process.execPath,
  [nextBin, "dev", "--webpack", "--port", String(PORT)],
  { CODEX_WEB_BRIDGE_PORT: BRIDGE_PORT },
);

function shutdown(code = 0) {
  bridge.kill("SIGTERM");
  next.kill("SIGTERM");
  process.exit(code);
}

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));

next.on("exit", (code) => shutdown(code ?? 0));
