import { NextResponse } from "next/server";
import { z } from "zod";
import { bridgeRpc } from "@/server/bridge";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

const StartTurnBody = z.object({
  text: z.string().min(1),
  // TODO: allow skill/app mentions + attachments
});

export async function POST(
  req: Request,
  ctx: { params: Promise<{ threadId: string }> },
) {
  try {
    const { threadId } = await ctx.params;
    const body = StartTurnBody.safeParse(await req.json().catch(() => null));
    if (!body.success) {
      return NextResponse.json(
        { ok: false, error: body.error.message },
        { status: 400 },
      );
    }

    const params = {
      threadId,
      input: [{ type: "text", text: body.data.text, text_elements: [] }],
    };

    // Avoid `thread/resume` unless we actually need it: calling it before every turn can
    // cause repeated prompt-prefix blocks to be appended into rollout history (wasted tokens).
    let result: unknown;
    try {
      result = await bridgeRpc("turn/start", params);
    } catch {
      // If the thread isn't currently loaded in app-server memory (e.g. after a restart),
      // resume and retry once.
      await bridgeRpc("thread/resume", { threadId }).catch(() => null);
      result = await bridgeRpc("turn/start", params);
    }

    return NextResponse.json({ ok: true, result });
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: String(err) },
      { status: 500 },
    );
  }
}
