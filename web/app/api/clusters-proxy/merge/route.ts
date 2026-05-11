import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * POST /api/clusters-proxy/merge
 *
 * Proxies POST /api/clusters/merge to the backend.
 * Body: { scope, canonical_id, absorb_ids }
 * The access token is read from the httpOnly cookie and forwarded
 * as Cookie: Authorization=Bearer <token>.
 */
export async function POST(request: NextRequest) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  let body: unknown;
  try {
    body = await request.json();
  } catch {
    return NextResponse.json({ detail: "Invalid JSON body" }, { status: 400 });
  }

  let upstream: Response;
  try {
    upstream = await fetch(`${API_BASE}/api/clusters/merge`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Cookie: `Authorization=Bearer ${accessToken}`,
      },
      body: JSON.stringify(body),
    });
  } catch (err) {
    console.error("[api/clusters-proxy/merge] POST fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 }
    );
  }

  const text = await upstream.text();
  return new NextResponse(text, {
    status: upstream.status,
    headers: { "Content-Type": "application/json" },
  });
}
