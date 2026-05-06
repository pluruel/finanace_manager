import { NextRequest, NextResponse } from "next/server";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

// MSA_INTEGRATION.md: /auth/refresh expects cookie transport (cookie wins over JSON body;
// JSON body input is removed). Forward local "refresh" cookie as "refresh_token" to auth-svc.
// Response is TokenPair — new refresh_token arrives via Set-Cookie; must be persisted or
// reuse-detection will revoke all sessions on the next rotation.
export async function POST(request: NextRequest) {
  const refreshToken = request.cookies.get("refresh")?.value;

  if (!refreshToken) {
    return NextResponse.json(
      { detail: "No refresh token" },
      { status: 401 },
    );
  }

  let authRes: Response;
  try {
    authRes = await fetch(`${AUTH_BASE_URL}/auth/refresh`, {
      method: "POST",
      headers: {
        // Cookie transport: auth-svc expects the cookie named "refresh_token"
        Cookie: `refresh_token=${refreshToken}`,
      },
    });
  } catch (err) {
    console.error("[auth/refresh] fetch error:", err);
    return NextResponse.json(
      { detail: "Auth service unavailable" },
      { status: 502 },
    );
  }

  if (!authRes.ok) {
    const text = await authRes.text().catch(() => "");
    return NextResponse.json(
      { detail: text || "Refresh failed" },
      { status: authRes.status },
    );
  }

  const tokenResp = (await authRes.json()) as {
    access_token: string;
    refresh_token?: string;
    token_type?: string;
    expires_in?: number;
  };

  if (!tokenResp.access_token) {
    return NextResponse.json(
      { detail: "Invalid refresh response" },
      { status: 502 },
    );
  }

  // Prefer Set-Cookie header (auth-svc rotates the cookie on every call).
  // Fall back to JSON body field (deprecated, present for backward compat only).
  let newRefreshToken = tokenResp.refresh_token;
  for (const cookieStr of authRes.headers.getSetCookie()) {
    const m = cookieStr.match(/^refresh_token=([^;]+)/);
    if (m) {
      newRefreshToken = m[1];
      break;
    }
  }

  if (!newRefreshToken) {
    return NextResponse.json(
      { detail: "Invalid refresh response" },
      { status: 502 },
    );
  }

  const isProduction = process.env.NODE_ENV === "production";

  const response = NextResponse.json({ ok: true });
  response.cookies.set("access", tokenResp.access_token, {
    httpOnly: true,
    secure: isProduction,
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 15,
  });
  response.cookies.set("refresh", newRefreshToken, {
    httpOnly: true,
    secure: isProduction,
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 60 * 24 * 14,
  });

  return response;
}
