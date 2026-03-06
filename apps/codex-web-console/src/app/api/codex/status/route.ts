import { NextResponse } from "next/server";
import { bridgeRpc } from "@/server/bridge";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET() {
  try {
    // Cheap call that should work even when logged out.
    const config = await bridgeRpc("config/read", { includeLayers: false });

    return NextResponse.json({
      ok: true,
      config: config?.result ?? config,
    });
  } catch (err) {
    return NextResponse.json(
      {
        ok: false,
        error: String(err),
      },
      { status: 500 },
    );
  }
}
