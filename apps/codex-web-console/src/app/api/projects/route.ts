import { NextResponse } from "next/server";
import { z } from "zod";
import { createProject, listProjects } from "@/server/store";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET() {
  return NextResponse.json({ ok: true, projects: await listProjects() });
}

const CreateProjectBody = z.object({
  path: z.string().min(1),
});

export async function POST(req: Request) {
  const body = CreateProjectBody.safeParse(await req.json().catch(() => null));
  if (!body.success) {
    return NextResponse.json(
      { ok: false, error: body.error.message },
      { status: 400 },
    );
  }

  const project = await createProject(body.data.path);
  return NextResponse.json({ ok: true, project });
}
