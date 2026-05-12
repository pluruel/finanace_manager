import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/** PATCH /api/payment-methods-proxy/:id/actor → backend PATCH /api/payment-methods/:id/actor */
export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }
  const { id } = await params;
  const body = await request.text();

  let backendRes: Response;
  try {
    backendRes = await fetch(
      `${API_BASE}/api/payment-methods/${encodeURIComponent(id)}/actor`,
      {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          Cookie: `Authorization=Bearer ${accessToken}`,
        },
        body,
      },
    );
  } catch (err) {
    console.error("[api/payment-methods-proxy] PATCH actor fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 },
    );
  }

  const text = await backendRes.text();
  return new NextResponse(text, {
    status: backendRes.status,
    headers: { "Content-Type": "application/json" },
  });
}
