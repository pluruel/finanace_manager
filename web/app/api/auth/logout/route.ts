import { NextRequest, NextResponse } from "next/server";
import { accessCookieOptions, refreshCookieOptions, appendLegacyRefreshDelete } from "@/lib/auth-cookies";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

/**
 * POST /api/auth/logout
 *
 * spec: cookie transport 우선 (JSON body deprecated)
 *
 * refresh 쿠키 값을 Cookie 헤더로 auth-svc /auth/logout에 전달하고 로컬 쿠키를 삭제한다.
 * MSA_INTEGRATION.md: POST /auth/logout은 cookie refresh_token 우선 (JSON body는 deprecated),
 *   204 응답, 항상 Set-Cookie: refresh_token=; Max-Age=0 발행.
 *
 * auth-svc 응답 성공/실패 무관하게 로컬 access/refresh 쿠키를 삭제하고 { ok: true } 반환.
 * fetch에는 2000ms 타임아웃을 적용한다 — 타임아웃 발생해도 로컬 쿠키 삭제는 그대로 진행.
 */
export async function POST(request: NextRequest) {
  const refreshToken = request.cookies.get("refresh")?.value;

  // auth-svc logout 시도 (실패해도 로컬 쿠키는 삭제)
  // Cookie 헤더로 refresh_token 전달 (spec: cookie wins over JSON body)
  // AbortSignal.timeout(2000): 2초 타임아웃 — 실패해도 로컬 쿠키 삭제는 진행
  if (refreshToken) {
    try {
      await fetch(`${AUTH_BASE_URL}/auth/logout`, {
        method: "POST",
        headers: {
          Cookie: `refresh_token=${refreshToken}`,
        },
        signal: AbortSignal.timeout(2000),
      });
    } catch (err) {
      console.error("[auth/logout] fetch error:", err);
      // 타임아웃(AbortError) 포함 모든 fetch 에러를 무시하고 로컬 쿠키 삭제 진행
    }
  }

  const isProduction = process.env.NODE_ENV === "production";
  const response = NextResponse.json({ ok: true });

  // 쿠키 삭제 (maxAge=0)
  response.cookies.set("access", "", {
    ...accessCookieOptions(isProduction, 0),
  });

  // refresh 쿠키 삭제 — path="/api/auth" 와 일치해야 삭제된다
  response.cookies.set("refresh", "", {
    ...refreshCookieOptions(isProduction, 0),
  });

  // 과도기 정리: 이전 배포에서 path="/" 로 발급된 stale refresh 쿠키 삭제
  // (NextResponse.cookies.set은 동일 이름을 덮어쓰므로 headers.append로 직접 추가)
  appendLegacyRefreshDelete(isProduction, response);

  return response;
}
