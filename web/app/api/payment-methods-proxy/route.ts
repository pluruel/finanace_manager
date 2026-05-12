import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * GET /api/payment-methods-proxy
 *
 * Proxies GET /api/payment-methods to the backend.
 * The access token is read from the httpOnly cookie and forwarded
 * as Cookie: Authorization=Bearer <token>.
 */
export async function GET(request: NextRequest) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  let upstream: Response;
  try {
    upstream = await fetch(`${API_BASE}/api/payment-methods`, {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
        Cookie: `Authorization=Bearer ${accessToken}`,
      },
      cache: "no-store",
    });
  } catch (err) {
    console.error("[api/payment-methods-proxy] GET fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 },
    );
  }

  const text = await upstream.text();
  return new NextResponse(text, {
    status: upstream.status,
    headers: { "Content-Type": "application/json" },
  });
}
