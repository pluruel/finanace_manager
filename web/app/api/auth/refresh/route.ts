import { NextRequest, NextResponse } from "next/server";
import { performRefresh } from "@/lib/perform-refresh";
import {
  accessCookieOptions,
  refreshCookieOptions,
  appendLegacyRefreshDelete,
} from "@/lib/auth-cookies";

// Re-export the type so existing imports from this path keep working.
export type { RefreshResult } from "@/lib/perform-refresh";

// ── Route 핸들러 ───────────────────────────────────────────────────────────────

/**
 * POST /api/auth/refresh
 *
 * performRefresh를 호출하고 결과를 NextResponse로 변환한다.
 * 비-OK 응답에는 access/refresh 쿠키 maxAge=0 삭제 헤더를 동반한다.
 */
export async function POST(request: NextRequest) {
  const refreshToken = request.cookies.get("refresh")?.value;
  const result = await performRefresh(refreshToken);

  const isProduction = process.env.NODE_ENV === "production";

  if (!result.ok) {
    let detail: string;
    if (result.status === 401) {
      detail = "Refresh token expired or revoked";
    } else if (result.status >= 500) {
      detail = "Authentication service unavailable";
    } else {
      detail = "Refresh failed";
    }

    const response = NextResponse.json({ detail }, { status: result.status });

    // 무효한 쿠키 삭제
    response.cookies.set("access", "", {
      ...accessCookieOptions(isProduction, 0),
    });
    response.cookies.set("refresh", "", {
      ...refreshCookieOptions(isProduction, 0),
    });

    // 과도기 정리: 이전 배포에서 path="/" 로 발급된 stale refresh 쿠키 삭제
    // (NextResponse.cookies.set은 동일 이름을 덮어쓰므로 headers.append로 직접 추가)
    appendLegacyRefreshDelete(isProduction, response);

    return response;
  }

  const response = NextResponse.json({ ok: true });

  for (const cookie of result.setCookies) {
    if (cookie.name === "access") {
      response.cookies.set(cookie.name, cookie.value, {
        ...accessCookieOptions(isProduction, cookie.maxAge),
      });
    } else if (cookie.name === "refresh") {
      response.cookies.set(cookie.name, cookie.value, {
        ...refreshCookieOptions(isProduction, cookie.maxAge),
      });
    }
  }

  return response;
}
