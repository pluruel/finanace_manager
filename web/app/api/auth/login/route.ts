import { NextRequest, NextResponse } from "next/server";

const AUTH_BASE_URL =
  process.env.AUTH_BASE_URL ?? "https://auth.junodevs.com";

/**
 * POST /api/auth/login
 *
 * 클라이언트에서 { username, password } JSON을 받아
 * auth-svc /auth/login에 form-urlencoded로 전달한다.
 * MSA_INTEGRATION.md: /auth/login은 application/x-www-form-urlencoded,
 *   username 필드에 email 값을 넣는다.
 *
 * 성공 시:
 *   - access 토큰 → httpOnly Secure SameSite=Lax 쿠키 "access"
 *   - refresh 토큰 → httpOnly Secure SameSite=Lax 쿠키 "refresh"
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
      { detail: "Auth service unavailable" },
      { status: 502 },
    );
  }

  if (!authRes.ok) {
    const text = await authRes.text().catch(() => "");
    return NextResponse.json(
      { detail: text || "Authentication failed" },
      { status: authRes.status },
    );
  }

  const tokenPair = (await authRes.json()) as {
    access_token: string;
    refresh_token: string;
    token_type?: string;
  };

  const { access_token, refresh_token } = tokenPair;

  if (!access_token || !refresh_token) {
    return NextResponse.json(
      { detail: "Invalid token response from auth service" },
      { status: 502 },
    );
  }

  // 프로덕션에서는 Secure가 필요하다.
  // 개발 환경(localhost)에서는 Secure를 false로 두어야 브라우저가 쿠키를 저장한다.
  const isProduction = process.env.NODE_ENV === "production";

  const response = NextResponse.json({ ok: true });

  // access 토큰 쿠키 — httpOnly로 클라이언트 JS 접근 차단
  response.cookies.set("access", access_token, {
    httpOnly: true,
    secure: isProduction,
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 15, // 15분 (auth-svc 기본 TTL)
  });

  // refresh 토큰 쿠키 — httpOnly, path="/"로 middleware에서 읽을 수 있게 설정
  // path="/api/auth"로 제한하면 middleware(루트 경로)에서 쿠키를 못 읽어 무한 리다이렉트 발생
  response.cookies.set("refresh", refresh_token, {
    httpOnly: true,
    secure: isProduction,
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 60 * 24 * 14, // 14일 (auth-svc 기본 TTL)
  });

  return response;
}
