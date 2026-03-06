import { NextResponse } from "next/server";
import { z } from "zod";
import { bridgeRespond } from "@/server/bridge";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

const ApprovalBody = z.object({
  id: z.string().min(1),
  decision: z.enum(["accept", "decline"]),
});

export async function POST(req: Request) {
  const body = ApprovalBody.safeParse(await req.json().catch(() => null));
  if (!body.success) {
    return NextResponse.json(
      { ok: false, error: body.error.message },
      { status: 400 },
    );
  }

  await bridgeRespond(body.data.id, { decision: body.data.decision });

  return NextResponse.json({ ok: true });
}
