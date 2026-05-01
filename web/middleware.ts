import { NextRequest, NextResponse } from "next/server";

/**
 * middleware.ts
 *
 * (app)/* 경로 진입 시:
 * 1. access 쿠키 존재 여부 확인
 * 2. 없거나 만료된 경우 → /api/auth/refresh 시도
 * 3. refresh 성공 → request.cookies에 새 access 쿠키 설정 후 NextResponse.next({ request })
 *    - NextResponse.next({ request }) 패턴으로 새 쿠키가 RSC까지 전파된다
 * 4. refresh 실패 → /login 리다이렉트
 *
 * access 토큰의 만료 여부는 쿠키의 유무로 판단한다.
 * (쿠키 maxAge가 토큰 TTL과 일치하도록 설정함)
 *
 * 참고: JWT를 클라이언트에서 디코딩하지 않는다 — 서버 전용.
 */

const PUBLIC_PATHS = ["/login", "/api/auth/login", "/api/auth/refresh", "/api/auth/logout"];

function isPublicPath(pathname: string): boolean {
  return PUBLIC_PATHS.some(
    (p) => pathname === p || pathname.startsWith(p + "/"),
  );
}

/**
 * Set-Cookie 헤더 문자열에서 name=value 부분만 추출한다.
 * 예: "access=abc123; Path=/; HttpOnly; Max-Age=900" → { name: "access", value: "abc123" }
 */
function parseCookieNameValue(setCookieStr: string): { name: string; value: string } | null {
  const firstPart = setCookieStr.split(";")[0]?.trim();
  if (!firstPart) return null;
  const eqIdx = firstPart.indexOf("=");
  if (eqIdx === -1) return null;
  return {
    name: firstPart.slice(0, eqIdx).trim(),
    value: firstPart.slice(eqIdx + 1).trim(),
  };
}

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;

  // 공개 경로는 통과
  if (isPublicPath(pathname)) {
    return NextResponse.next();
  }

  // 정적 파일, Next.js 내부 경로 통과
  if (
    pathname.startsWith("/_next") ||
    pathname.startsWith("/favicon") ||
    pathname.includes(".")
  ) {
    return NextResponse.next();
  }

  const accessToken = request.cookies.get("access")?.value;

  // access 토큰이 있으면 통과
  if (accessToken) {
    return NextResponse.next();
  }

  // access 토큰 없음 → refresh 시도
  const refreshToken = request.cookies.get("refresh")?.value;

  if (!refreshToken) {
    // refresh 토큰도 없으면 로그인으로
    const loginUrl = new URL("/login", request.url);
    loginUrl.searchParams.set("from", pathname);
    return NextResponse.redirect(loginUrl);
  }

  // 같은 origin의 /api/auth/refresh를 호출한다
  const refreshUrl = new URL("/api/auth/refresh", request.url);

  let refreshRes: Response;
  try {
    refreshRes = await fetch(refreshUrl.toString(), {
      method: "POST",
      headers: {
        // refresh 쿠키를 포함해서 전달
        Cookie: request.headers.get("cookie") ?? "",
      },
    });
  } catch {
    const loginUrl = new URL("/login", request.url);
    return NextResponse.redirect(loginUrl);
  }

  if (!refreshRes.ok) {
    // refresh 실패 → 로그인으로
    const loginUrl = new URL("/login", request.url);
    return NextResponse.redirect(loginUrl);
  }

  // getSetCookie()로 multi Set-Cookie 헤더를 배열로 수집 (Next 15 / Fetch API 표준)
  // headers.get('set-cookie')는 복수 쿠키를 콤마로 합쳐 깨지므로 사용하지 않는다.
  // Next 15는 getSetCookie()를 보장하므로 falsy 가드 없이 직접 호출한다.
  const setCookies: string[] = refreshRes.headers.getSetCookie();

  // refresh 응답에 Set-Cookie가 없으면 비정상 응답 → 로그인으로 리다이렉트
  if (setCookies.length === 0) {
    const loginUrl = new URL("/login", request.url);
    return NextResponse.redirect(loginUrl);
  }

  // 새 쿠키 값을 request.cookies에 반영해 RSC까지 전파한다.
  // NextResponse.next({ request })로 수정된 request 헤더가 downstream까지 전달된다.
  for (const cookieStr of setCookies) {
    const parsed = parseCookieNameValue(cookieStr);
    if (parsed) {
      request.cookies.set(parsed.name, parsed.value);
    }
  }

  const response = NextResponse.next({ request });

  // 브라우저에도 각 Set-Cookie를 개별 헤더로 추가한다.
  // append를 사용해 복수 Set-Cookie 헤더를 유지한다.
  for (const cookieStr of setCookies) {
    response.headers.append("Set-Cookie", cookieStr);
  }

  return response;
}

// 런타임 가정: Node.js (same-origin fetch + getSetCookie() 사용).
// Next 15 middleware는 기본 Edge runtime이나, getSetCookie()는 Web Fetch API 표준이므로 Edge에서도 동작한다.
// same-origin fetch(request.url 기반)는 Edge에서도 지원된다.
// 단, Node.js 전용 API(fs, crypto 등)를 추가할 경우 반드시 런타임을 Node.js로 명시해야 한다.
export const config = {
  matcher: [
    /*
     * 다음을 제외한 모든 경로에 적용:
     * - _next/static (정적 파일)
     * - _next/image (이미지 최적화)
     * - favicon.ico
     */
    "/((?!_next/static|_next/image|favicon.ico).*)",
  ],
};
