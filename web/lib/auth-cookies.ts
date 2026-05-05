/**
 * auth-cookies.ts
 *
 * 인증 쿠키 관련 공통 헬퍼.
 * login / refresh / logout route 핸들러와 middleware가 공유한다.
 */

// ── Set-Cookie 파서 ────────────────────────────────────────────────────────────

/**
 * Set-Cookie 헤더 문자열에서 name=value 부분만 추출한다.
 * 예: "access=abc123; Path=/; HttpOnly; Max-Age=900" → { name: "access", value: "abc123" }
 */
export function parseSetCookieNameValue(
  s: string,
): { name: string; value: string } | null {
  const firstPart = s.split(";")[0]?.trim();
  if (!firstPart) return null;
  const eqIdx = firstPart.indexOf("=");
  if (eqIdx === -1) return null;
  return {
    name: firstPart.slice(0, eqIdx).trim(),
    value: firstPart.slice(eqIdx + 1).trim(),
  };
}

// ── 쿠키 옵션 타입 ─────────────────────────────────────────────────────────────

export interface CookieOptions {
  httpOnly: true;
  secure: boolean;
  sameSite: "lax";
  path: string;
  maxAge: number;
}

// ── 쿠키 옵션 빌더 ─────────────────────────────────────────────────────────────

/**
 * access 쿠키 옵션 (path="/")
 * maxAge: 초 단위. 기본 15분 fallback.
 */
export function accessCookieOptions(
  isProd: boolean,
  maxAge: number = 60 * 15,
): CookieOptions {
  return {
    httpOnly: true,
    secure: isProd,
    sameSite: "lax",
    path: "/",
    maxAge,
  };
}

/**
 * refresh 쿠키 옵션 (path="/api/auth" — 인증 엔드포인트에만 전송)
 * maxAge: 초 단위. 기본 14일 fallback.
 */
export function refreshCookieOptions(
  isProd: boolean,
  maxAge: number = 60 * 60 * 24 * 14,
): CookieOptions {
  return {
    httpOnly: true,
    secure: isProd,
    sameSite: "lax",
    path: "/api/auth",
    maxAge,
  };
}

/**
 * 과도기 정리 전용: 이전 배포에서 path="/" 로 발급된 stale refresh 쿠키 삭제용.
 *
 * 브라우저는 path가 다르면 별개 쿠키로 취급하므로, path="/api/auth" 삭제 헤더만으로는
 * 옛 path="/" 쿠키를 지울 수 없다. 이 헬퍼는 삭제 분기에서만 추가로 발행한다.
 *
 * NextResponse.cookies.set은 같은 이름의 쿠키를 덮어쓰므로, raw Set-Cookie 헤더를
 * response.headers.append로 직접 추가해야 두 줄이 모두 발행된다.
 *
 * 주의: refresh 쿠키 발급(신규 생성) 시에는 절대 사용하지 않는다.
 *
 * @param isProd NODE_ENV === 'production' 여부 (Secure 속성 제어)
 * @param response Set-Cookie 헤더를 append할 NextResponse 인스턴스
 */
export function appendLegacyRefreshDelete(
  isProd: boolean,
  response: { headers: { append: (name: string, value: string) => void } },
): void {
  const secure = isProd ? "; Secure" : "";
  response.headers.append(
    "Set-Cookie",
    `refresh=; Path=/; Max-Age=0; HttpOnly; SameSite=lax${secure}`,
  );
}
