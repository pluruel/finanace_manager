import { NextRequest, NextResponse } from "next/server";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

/**
 * POST /api/auth/logout
 *
 * refresh 쿠키를 auth-svc /auth/logout에 전달(선택적)하고 쿠키를 삭제한다.
 * MSA_INTEGRATION.md: POST /auth/logout body: JSON { refresh_token } (optional), 204
 */
export async function POST(request: NextRequest) {
  const refreshToken = request.cookies.get("refresh")?.value;

  // auth-svc logout 시도 (실패해도 로컬 쿠키는 삭제)
  if (refreshToken) {
    try {
      await fetch(`${AUTH_BASE_URL}/auth/logout`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
    } catch (err) {
      console.error("[auth/logout] fetch error:", err);
    }
  }

  const response = NextResponse.json({ ok: true });

  // 쿠키 삭제 (maxAge=0)
  response.cookies.set("access", "", {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: 0,
  });

  // path는 login 시 설정한 "/" 와 일치해야 삭제된다
  response.cookies.set("refresh", "", {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: 0,
  });

  return response;
}
