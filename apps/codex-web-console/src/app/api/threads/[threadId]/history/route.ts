import { NextResponse } from "next/server";
import { bridgeRpc } from "@/server/bridge";
import { parseRolloutJsonlFile } from "@/server/rollout";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET(
  _req: Request,
  ctx: { params: Promise<{ threadId: string }> },
) {
  const { threadId } = await ctx.params;
  const read = await bridgeRpc("thread/read", { threadId, includeTurns: false });
  const thread =
    read && typeof read === "object" && read !== null && "result" in read
      ? (read as { result?: { thread?: unknown } }).result?.thread
      : null;

  const rolloutPath =
    thread &&
    typeof thread === "object" &&
    thread !== null &&
    "path" in (thread as Record<string, unknown>) &&
    typeof (thread as Record<string, unknown>).path === "string"
      ? ((thread as Record<string, unknown>).path as string)
      : null;

  if (!rolloutPath) return NextResponse.json({ ok: true, events: [] });

  const events = await parseRolloutJsonlFile(rolloutPath, 3000);
  return NextResponse.json({ ok: true, events });
}
