import { NextResponse } from "next/server";
import { z } from "zod";
import { bridgeRpc } from "@/server/bridge";
import {
  getProject,
  type ProjectId,
  upsertThread,
} from "@/server/store";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET(
  _req: Request,
  ctx: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await ctx.params;
  const project = await getProject(projectId as ProjectId);
  if (!project) {
    return NextResponse.json(
      { ok: false, error: "Project not found" },
      { status: 404 },
    );
  }

  const normalizedProjectCwd = project.path
    .replaceAll("\\", "/")
    .replace(/\/+$/, "")
    .toLowerCase();

  const threads: Array<{
    id: string;
    preview: string;
    createdAt: number;
    updatedAt: number;
    cwd: string;
  }> = [];

  let cursor: string | null = null;
  for (let i = 0; i < 5; i++) {
    const page = await bridgeRpc("thread/list", {
      cursor,
      limit: 50,
      sortKey: "updated_at",
      // Keep default sources; we only filter by cwd.
    });

    const data =
      typeof page === "object" && page !== null && "result" in page
        ? (page as { result?: { data?: unknown } }).result?.data
        : null;
    const next =
      typeof page === "object" && page !== null && "result" in page
        ? (page as { result?: { nextCursor?: unknown } }).result?.nextCursor
        : null;
    if (Array.isArray(data)) {
      for (const t of data) {
        if (!t || typeof t !== "object") continue;
        const cwd =
          "cwd" in (t as Record<string, unknown>) &&
          typeof (t as Record<string, unknown>).cwd === "string"
            ? ((t as Record<string, unknown>).cwd as string)
            : "";
        const normalizedCwd = cwd
          .replaceAll("\\", "/")
          .replace(/\/+$/, "")
          .toLowerCase();
        if (normalizedCwd !== normalizedProjectCwd) continue;
        threads.push({
          id:
            "id" in (t as Record<string, unknown>)
              ? String((t as Record<string, unknown>).id)
              : "",
          preview:
            "preview" in (t as Record<string, unknown>)
              ? String((t as Record<string, unknown>).preview ?? "")
              : "",
          createdAt:
            "createdAt" in (t as Record<string, unknown>)
              ? Number((t as Record<string, unknown>).createdAt ?? 0)
              : 0,
          updatedAt:
            "updatedAt" in (t as Record<string, unknown>)
              ? Number((t as Record<string, unknown>).updatedAt ?? 0)
              : 0,
          cwd,
        });
      }
    }
    cursor = typeof next === "string" ? next : null;
    if (!cursor) break;
  }

  return NextResponse.json({ ok: true, threads });
}

const CreateThreadBody = z.object({
  name: z.string().min(1).optional(),
});

export async function POST(
  req: Request,
  ctx: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await ctx.params;
  const project = await getProject(projectId as ProjectId);
  if (!project) {
    return NextResponse.json(
      { ok: false, error: "Project not found" },
      { status: 404 },
    );
  }

  const body = CreateThreadBody.safeParse(await req.json().catch(() => null));
  if (!body.success) {
    return NextResponse.json(
      { ok: false, error: body.error.message },
      { status: 400 },
    );
  }

  const result = await bridgeRpc("thread/start", {
    cwd: project.path,
    experimentalRawEvents: true,
  });

  const threadId =
    typeof result === "object" &&
    result !== null &&
    "result" in result &&
    (result as { result?: unknown }).result &&
    typeof (result as { result: unknown }).result === "object" &&
    (result as { result: object }).result !== null &&
    "thread" in ((result as { result: Record<string, unknown> }).result) &&
    typeof ((result as { result: Record<string, unknown> }).result.thread) === "object" &&
    ((result as { result: Record<string, unknown> }).result.thread) !== null &&
    "id" in (((result as { result: Record<string, unknown> }).result.thread) as Record<string, unknown>) &&
    typeof ((((result as { result: Record<string, unknown> }).result.thread) as Record<string, unknown>).id) === "string"
      ? String(
          (((result as { result: Record<string, unknown> }).result.thread) as Record<
            string,
            unknown
          >).id,
        )
      : null;

  if (!threadId) {
    return NextResponse.json(
      { ok: false, error: "Unexpected thread/start response" },
      { status: 502 },
    );
  }

  const ref = await upsertThread({
    id: threadId,
    projectId: project.id,
    createdAt: Date.now(),
    name: body.data.name ?? null,
  });

  if (body.data.name) {
    // best-effort: set server-side name
    await bridgeRpc("thread/name/set", { threadId, name: body.data.name }).catch(
      () => null,
    );
  }

  return NextResponse.json({ ok: true, thread: ref });
}
