import { NextResponse } from "next/server";
import { bridgeConfig } from "@/server/bridge";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET() {
  const { baseUrl } = bridgeConfig();
  const res = await fetch(`${baseUrl}/debug`, { cache: "no-store" });
  const json = await res.json().catch(() => null);
  return NextResponse.json(json ?? { ok: false, error: "bridge debug failed" });
}
