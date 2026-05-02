import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * POST /api/entities-proxy/:scope/:id/confirm
 *
 * Proxies POST /api/entities/:scope/:id/confirm to the backend.
 * The access token is read from the httpOnly cookie so it never reaches the client.
 */
export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ scope: string; id: string }> },
) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  const { scope, id } = await params;

  let backendRes: Response;
  try {
    backendRes = await fetch(
      `${API_BASE}/api/entities/${encodeURIComponent(scope)}/${encodeURIComponent(id)}/confirm`,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Cookie: `Authorization=Bearer ${accessToken}`,
        },
      },
    );
  } catch (err) {
    console.error("[api/entities-proxy] POST confirm fetch error:", err);
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
