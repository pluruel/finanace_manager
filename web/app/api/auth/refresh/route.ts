import { NextRequest, NextResponse } from "next/server";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

/**
 * POST /api/auth/refresh
 *
 * refresh 쿠키를 읽어 auth-svc /auth/refresh에 JSON으로 전달한다.
 * MSA_INTEGRATION.md: POST /auth/refresh body: JSON { refresh_token }
 * 응답: AccessTokenResp (access_token만 재발급, refresh 변경 없음)
 *
 * 성공 시 새 access 토큰을 httpOnly 쿠키로 갱신한다.
 */
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
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ refresh_token: refreshToken }),
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
    token_type?: string;
  };

  if (!tokenResp.access_token) {
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

  return response;
}
