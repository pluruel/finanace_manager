/**
 * perform-refresh.ts
 *
 * refresh 핵심 로직 — 순수 함수.
 * middleware와 /api/auth/refresh route 핸들러 양쪽에서 직접 import해 호출한다.
 * (middleware의 self-fetch 제거)
 *
 * BREAKING CHANGE 적용 (MSA_INTEGRATION.md §"Refresh-token rotation"):
 *   - auth-svc /auth/refresh는 이제 TokenPair를 반환한다.
 *   - 제시한 refresh 토큰은 즉시 revoke된다.
 *   - 응답의 새 refresh_token만 유효하다.
 *
 * Set-Cookie 화이트리스트:
 *   - name이 "access" 또는 "refresh"인 쿠키만 setCookies에 포함.
 *   - auth-svc 응답에 다른 이름의 쿠키가 섞여도 무시한다.
 */

import { parseSetCookieNameValue } from "@/lib/auth-cookies";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

// ── 허용된 쿠키 이름 화이트리스트 ─────────────────────────────────────────────────

const ALLOWED_COOKIE_NAMES = new Set<string>(["access", "refresh"]);

// ── 반환 타입 ──────────────────────────────────────────────────────────────────

export type RefreshResult =
  | {
      ok: true;
      setCookies: {
        name: "access" | "refresh";
        value: string;
        maxAge: number;
      }[];
    }
  | { ok: false; status: number };

/**
 * performRefresh
 *
 * @param refreshToken  우리 "refresh" 쿠키 값 (undefined이면 즉시 401)
 */
export async function performRefresh(
  refreshToken: string | undefined,
): Promise<RefreshResult> {
  if (!refreshToken) {
    return { ok: false, status: 401 };
  }

  let authRes: Response;
  try {
    authRes = await fetch(`${AUTH_BASE_URL}/auth/refresh`, {
      method: "POST",
      headers: {
        Cookie: `refresh_token=${refreshToken}`,
      },
    });
  } catch (err) {
    console.error("[auth/refresh] fetch error:", err);
    return { ok: false, status: 502 };
  }

  // 401: 만료 또는 reuse detection
  if (authRes.status === 401) {
    return { ok: false, status: 401 };
  }

  if (!authRes.ok) {
    // 에러 본문 마스킹
    if (authRes.status >= 500) {
      return { ok: false, status: 502 };
    }
    // 4xx — detail 필드만 추출
    return { ok: false, status: authRes.status };
  }

  // TokenPair 응답 파싱
  let tokenPair: {
    access_token: string;
    refresh_token?: string;
    token_type?: string;
    expires_in?: number;
  };
  try {
    tokenPair = (await authRes.json()) as typeof tokenPair;
  } catch {
    return { ok: false, status: 502 };
  }

  if (!tokenPair.access_token) {
    return { ok: false, status: 502 };
  }

  const accessMaxAge = tokenPair.expires_in ?? 60 * 15;

  // 회전된 refresh_token 추출: Set-Cookie 헤더 우선, fallback으로 JSON 필드
  // getSetCookie()는 Next 15 / Web Fetch API 표준 — 복수 Set-Cookie 헤더를 배열로 반환.
  const rawSetCookies: string[] = authRes.headers.getSetCookie();
  let newRefreshToken: string | undefined;

  for (const cookieStr of rawSetCookies) {
    const parsed = parseSetCookieNameValue(cookieStr);
    if (parsed?.name === "refresh_token") {
      newRefreshToken = parsed.value;
      break;
    }
  }

  // Set-Cookie에 없으면 JSON 필드 fallback (deprecated)
  if (!newRefreshToken) {
    newRefreshToken = tokenPair.refresh_token;
  }

  // 결과 쿠키 목록 구성 (화이트리스트: "access" | "refresh" 만 허용)
  type CookieEntry = { name: "access" | "refresh"; value: string; maxAge: number };
  const setCookies: CookieEntry[] = [];

  // access 쿠키
  setCookies.push({
    name: "access",
    value: tokenPair.access_token,
    maxAge: accessMaxAge,
  });

  // refresh 쿠키 (새 토큰이 있을 때만)
  if (newRefreshToken) {
    setCookies.push({
      name: "refresh",
      value: newRefreshToken,
      maxAge: 60 * 60 * 24 * 14,
    });
  }

  // 화이트리스트 검증 (런타임 안전망)
  const filteredCookies = setCookies.filter((c) =>
    ALLOWED_COOKIE_NAMES.has(c.name),
  );

  return { ok: true, setCookies: filteredCookies };
}
