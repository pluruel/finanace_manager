import { NextRequest, NextResponse } from "next/server";
import { performRefresh } from "@/lib/perform-refresh";
import { accessCookieOptions, refreshCookieOptions, appendLegacyRefreshDelete } from "@/lib/auth-cookies";

/**
 * middleware.ts
 *
 * (app)/* 경로 진입 시:
 * 1. access 쿠키 존재 여부 확인
 * 2. 없으면 performRefresh() 직접 호출 (self-fetch 제거)
 * 3. refresh 성공 → request.cookies에 새 access/refresh 쿠키 설정 후 NextResponse.next({ request })
 *    - NextResponse.next({ request }) 패턴으로 새 쿠키가 RSC까지 전파된다
 * 4. refresh 실패 → access/refresh 쿠키 삭제 헤더를 추가한 뒤 /login?from=... 리다이렉트
 *
 * access 토큰의 만료 여부는 쿠키의 유무로 판단한다.
 * (쿠키 maxAge가 토큰 TTL과 일치하도록 설정함)
 *
 * refresh 쿠키는 path="/api/auth"로 좁혀져 있지만, NextRequest.cookies는
 * 단일 Cookie 요청 헤더를 path 무관하게 파싱하므로 미들웨어에서 읽힌다.
 *
 * 참고: JWT를 클라이언트에서 디코딩하지 않는다 — 서버 전용.
 */

const PUBLIC_PATHS = ["/login", "/api/auth/login", "/api/auth/refresh", "/api/auth/logout"];

function isPublicPath(pathname: string): boolean {
  return PUBLIC_PATHS.some(
    (p) => pathname === p || pathname.startsWith(p + "/"),
  );
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

  // access 토큰 없음 → performRefresh 직접 호출 (self-fetch 없음)
  // NextRequest.cookies는 Cookie 헤더를 path 무관하게 파싱하므로
  // refresh 쿠키가 path="/api/auth" 로 좁혀져 있어도 여기서 읽힌다.
  const refreshToken = request.cookies.get("refresh")?.value;
  const result = await performRefresh(refreshToken);

  const isProduction = process.env.NODE_ENV === "production";

  if (!result.ok) {
    // refresh 실패 (401 포함) → stale 쿠키 삭제 후 /login?from=... 리다이렉트
    const loginUrl = new URL("/login", request.url);
    loginUrl.searchParams.set("from", pathname);
    const redirectResponse = NextResponse.redirect(loginUrl);

    redirectResponse.cookies.set("access", "", {
      ...accessCookieOptions(isProduction, 0),
    });
    redirectResponse.cookies.set("refresh", "", {
      ...refreshCookieOptions(isProduction, 0),
    });

    // 과도기 정리: 이전 배포에서 path="/" 로 발급된 stale refresh 쿠키 삭제
    // (NextResponse.cookies.set은 동일 이름을 덮어쓰므로 headers.append로 직접 추가)
    appendLegacyRefreshDelete(isProduction, redirectResponse);

    return redirectResponse;
  }

  // refresh 성공 — 새 쿠키를 request.cookies에 반영해 RSC까지 전파
  for (const cookie of result.setCookies) {
    request.cookies.set(cookie.name, cookie.value);
  }

  const response = NextResponse.next({ request });

  // 브라우저에도 새 쿠키 set
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
