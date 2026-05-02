import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * POST /api/aliases-proxy
 *
 * Proxies alias actions from the client to the backend.
 * Supports two actions via the JSON body:
 *   { action: "merge", scope, raw_text, target_id } → POST /api/aliases
 *
 * The access token is read from the httpOnly cookie and forwarded
 * as Cookie: Authorization=Bearer <token>. The client never sees the token.
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

  const { action, scope, raw_text, target_id } = body as {
    action?: string;
    scope?: string;
    raw_text?: string;
    target_id?: string;
  };

  if (action === "merge") {
    if (!scope || !raw_text || !target_id) {
      return NextResponse.json(
        { detail: "scope, raw_text, and target_id are required" },
        { status: 400 },
      );
    }

    let backendRes: Response;
    try {
      backendRes = await fetch(`${API_BASE}/api/aliases`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Cookie: `Authorization=Bearer ${accessToken}`,
        },
        body: JSON.stringify({ scope, raw_text, target_id }),
      });
    } catch (err) {
      console.error("[api/aliases-proxy] POST fetch error:", err);
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

  return NextResponse.json({ detail: `Unknown action: ${action ?? "(none)"}` }, { status: 400 });
}

/**
 * DELETE /api/aliases-proxy?id=<alias_id>
 *
 * Proxies DELETE /api/aliases/:id to the backend.
 */
export async function DELETE(request: NextRequest) {
  const accessToken = request.cookies.get("access")?.value;
  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  const aliasId = request.nextUrl.searchParams.get("id");
  if (!aliasId) {
    return NextResponse.json({ detail: "id query param required" }, { status: 400 });
  }

  let backendRes: Response;
  try {
    backendRes = await fetch(`${API_BASE}/api/aliases/${encodeURIComponent(aliasId)}`, {
      method: "DELETE",
      headers: {
        Cookie: `Authorization=Bearer ${accessToken}`,
      },
    });
  } catch (err) {
    console.error("[api/aliases-proxy] DELETE fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 },
    );
  }

  // 204 No Content — forward as-is
  return new NextResponse(null, { status: backendRes.status });
}
