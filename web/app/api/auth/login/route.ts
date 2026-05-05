import { NextRequest, NextResponse } from "next/server";
import {
  parseSetCookieNameValue,
  accessCookieOptions,
  refreshCookieOptions,
} from "@/lib/auth-cookies";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

/**
 * POST /api/auth/login
 *
 * spec: TokenPair, cookie transport 우선
 *
 * 클라이언트에서 { username, password } JSON을 받아
 * auth-svc /auth/login에 form-urlencoded로 전달한다.
 * MSA_INTEGRATION.md: /auth/login은 application/x-www-form-urlencoded,
 *   username 필드에 email 값을 넣는다.
 *
 * refresh_token 추출 우선순위:
 *   1. auth-svc 응답의 Set-Cookie 헤더에서 name==="refresh_token" 쿠키 값
 *   2. 없으면 JSON 응답의 refresh_token 필드 (deprecated — 향후 제거 예정)
 *
 * auth-svc Set-Cookie를 forward하지 않고, 우리 도메인의 access/refresh 쿠키를
 * 직접 set한다. name이 "access"/"refresh"인 쿠키만 우리가 set (화이트리스트).
 *
 * 에러 마스킹:
 *   - 5xx → 502 + "Authentication service unavailable"
 *   - 4xx → detail 필드만 추출해 동일 상태코드로 반환
 *
 * 성공 시:
 *   - access 토큰 → httpOnly Secure(prod) SameSite=Lax 쿠키 "access" (path="/", maxAge=expires_in ?? 15분)
 *   - refresh 토큰 → httpOnly Secure(prod) SameSite=Lax 쿠키 "refresh" (path="/api/auth", maxAge 14일)
 *   - 클라이언트에는 { ok: true }만 반환 (토큰 노출 금지)
 */
export async function POST(request: NextRequest) {
  let body: { username: string; password: string };
  try {
    body = (await request.json()) as { username: string; password: string };
  } catch {
    return NextResponse.json({ detail: "Invalid request body" }, { status: 400 });
  }

  const { username, password } = body;

  if (!username || !password) {
    return NextResponse.json(
      { detail: "username and password are required" },
      { status: 400 },
    );
  }

  // form-urlencoded 형식으로 auth-svc에 전달
  const formData = new URLSearchParams();
  formData.append("username", username); // username 필드에 email 값
  formData.append("password", password);

  let authRes: Response;
  try {
    authRes = await fetch(`${AUTH_BASE_URL}/auth/login`, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
      },
      body: formData.toString(),
    });
  } catch (err) {
    console.error("[auth/login] fetch error:", err);
    return NextResponse.json(
      { detail: "Authentication service unavailable" },
      { status: 502 },
    );
  }

  if (!authRes.ok) {
    // 5xx → 502 + generic 메시지 (auth-svc 내부 상세 노출 금지)
    if (authRes.status >= 500) {
      return NextResponse.json(
        { detail: "Authentication service unavailable" },
        { status: 502 },
      );
    }
    // 4xx → detail 필드만 추출
    let detail = "Authentication failed";
    try {
      const parsed = (await authRes.json()) as { detail?: string };
      if (typeof parsed.detail === "string" && parsed.detail) {
        detail = parsed.detail;
      }
    } catch {
      // JSON 파싱 실패 시 generic 메시지 유지
    }
    return NextResponse.json({ detail }, { status: authRes.status });
  }

  const tokenPair = (await authRes.json()) as {
    access_token: string;
    refresh_token?: string; // deprecated — Set-Cookie 우선
    token_type?: string;
    expires_in?: number;
  };

  const { access_token } = tokenPair;

  if (!access_token) {
    return NextResponse.json(
      { detail: "Invalid token response from auth service" },
      { status: 502 },
    );
  }

  // refresh_token 추출: Set-Cookie 헤더 우선, fallback으로 JSON 필드
  // getSetCookie()는 Next 15 / Web Fetch API 표준 — 복수 Set-Cookie 헤더를 배열로 반환.
  // headers.get('set-cookie')로 콤마 합치기 금지.
  const setCookies: string[] = authRes.headers.getSetCookie();
  let refresh_token: string | undefined;

  for (const cookieStr of setCookies) {
    const parsed = parseSetCookieNameValue(cookieStr);
    if (parsed?.name === "refresh_token") {
      refresh_token = parsed.value;
      break;
    }
  }

  // Set-Cookie에 없으면 JSON 필드 fallback (deprecated)
  if (!refresh_token) {
    refresh_token = tokenPair.refresh_token;
  }

  if (!refresh_token) {
    return NextResponse.json(
      { detail: "Invalid token response from auth service" },
      { status: 502 },
    );
  }

  const isProduction = process.env.NODE_ENV === "production";
  // expires_in을 access 쿠키 maxAge로 사용. 없으면 15분 fallback.
  const accessMaxAge = tokenPair.expires_in ?? 60 * 15;

  const response = NextResponse.json({ ok: true });

  // auth-svc Set-Cookie를 forward하지 않고 우리 도메인 쿠키를 직접 set (화이트리스트)
  response.cookies.set("access", access_token, {
    ...accessCookieOptions(isProduction, accessMaxAge),
  });

  response.cookies.set("refresh", refresh_token, {
    ...refreshCookieOptions(isProduction),
  });

  return response;
}
