import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * GET /api/products-proxy
 *
 * Proxies GET /api/products to the backend.
 * The access token is read from the httpOnly cookie and forwarded
 * as Cookie: Authorization=Bearer <token>.
 */
export async function GET(request: NextRequest) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  const upstream = await fetch(
    `${API_BASE}/api/products${request.nextUrl.search}`,
    {
      headers: { Cookie: `Authorization=Bearer ${accessToken}` },
      cache: "no-store",
    },
  ).catch(() => null);

  if (!upstream) {
    return NextResponse.json({ detail: "Upstream unreachable" }, { status: 502 });
  }

  const text = await upstream.text();
  return new NextResponse(text, {
    status: upstream.status,
    headers: { "Content-Type": "application/json" },
  });
}
