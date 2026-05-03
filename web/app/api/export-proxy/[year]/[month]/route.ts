import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * GET /api/export-proxy/:year/:month
 *
 * Proxies the backend xlsx export to the browser. Forwards the access cookie
 * as Cookie: Authorization=Bearer so the access token never reaches the client
 * bundle.
 */
export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ year: string; month: string }> },
) {
  const { year, month } = await params;
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  if (!/^\d{4}$/.test(year) || !/^\d{1,2}$/.test(month)) {
    return NextResponse.json({ detail: "Invalid year or month" }, { status: 400 });
  }

  let backendRes: Response;
  try {
    backendRes = await fetch(`${API_BASE}/api/export/${year}/${month}`, {
      method: "GET",
      headers: { Cookie: `Authorization=Bearer ${accessToken}` },
      cache: "no-store",
    });
  } catch (err) {
    console.error("[api/export-proxy] fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 },
    );
  }

  if (!backendRes.ok) {
    const text = await backendRes.text();
    return new NextResponse(text, { status: backendRes.status });
  }

  const headers = new Headers();
  const contentType =
    backendRes.headers.get("content-type") ??
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
  headers.set("Content-Type", contentType);
  const cd =
    backendRes.headers.get("content-disposition") ??
    `attachment; filename="finance-${year}-${String(month).padStart(2, "0")}.xlsx"`;
  headers.set("Content-Disposition", cd);

  return new NextResponse(backendRes.body, { status: 200, headers });
}
