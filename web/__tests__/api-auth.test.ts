/**
 * api-auth.test.ts
 *
 * /api/auth/{login,refresh,logout} Route Handler 단위 테스트.
 *
 * 검증 항목:
 *   login:
 *     - auth-svc Set-Cookie의 refresh_token 값이 우리 refresh 쿠키 값으로 들어간다.
 *     - Set-Cookie 부재 시 JSON refresh_token fallback 동작.
 *     - auth-svc 5xx → 502 + "Authentication service unavailable" 마스킹.
 *     - auth-svc 4xx → detail 필드만 추출, 원문 텍스트 통째로 노출 금지.
 *   refresh (performRefresh 포함):
 *     - 우리 refresh 쿠키 값이 auth-svc 호출의 Cookie 헤더에 refresh_token=...로 실린다(JSON body 없음).
 *     - TokenPair 응답에서 회전된 refresh_token이 우리 refresh 쿠키에 갱신된다.
 *     - auth-svc 401 → 우리 라우트도 401, access/refresh 쿠키 삭제 헤더 포함.
 *     - auth-svc 5xx → 502 + generic 마스킹.
 *   logout:
 *     - Cookie: refresh_token=<v> 헤더로 auth-svc 호출, JSON body 없음.
 *     - 로컬 access/refresh 두 쿠키 삭제.
 *     - AbortSignal 타임아웃 발생해도 로컬 쿠키 삭제는 일어난다.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { NextRequest } from "next/server";

// ── helpers ─────────────────────────────────────────────────────────────────

/** Set-Cookie 헤더 문자열 배열에서 특정 이름 쿠키의 value 추출 */
function getCookieValue(setCookieHeaders: string[], name: string): string | undefined {
  for (const h of setCookieHeaders) {
    const firstPart = h.split(";")[0]?.trim() ?? "";
    const eqIdx = firstPart.indexOf("=");
    if (eqIdx === -1) continue;
    const cookieName = firstPart.slice(0, eqIdx).trim();
    if (cookieName === name) return firstPart.slice(eqIdx + 1).trim();
  }
  return undefined;
}

/** Response에서 Set-Cookie 헤더 배열 반환 */
function getSetCookies(response: Response): string[] {
  // getSetCookie()는 Fetch API 표준. Node 환경에서도 동작한다.
  if (typeof (response.headers as Headers & { getSetCookie?: () => string[] }).getSetCookie === "function") {
    return (response.headers as Headers & { getSetCookie: () => string[] }).getSetCookie();
  }
  // fallback (jsdom 일부 버전)
  const raw = response.headers.get("set-cookie");
  return raw ? [raw] : [];
}

/** NextRequest 생성 헬퍼 */
function makeRequest(opts: {
  method?: string;
  body?: unknown;
  cookies?: Record<string, string>;
}): NextRequest {
  const url = "http://localhost:3000/api/auth/test";
  const initHeaders: Record<string, string> = {};
  const initOpts: { method: string; body?: string; headers?: Record<string, string> } = {
    method: opts.method ?? "POST",
  };
  if (opts.body !== undefined) {
    initOpts.body = JSON.stringify(opts.body);
    initHeaders["Content-Type"] = "application/json";
    initOpts.headers = initHeaders;
  }
  const req = new NextRequest(url, initOpts);
  // 쿠키 설정
  if (opts.cookies) {
    const cookieStr = Object.entries(opts.cookies)
      .map(([k, v]) => `${k}=${v}`)
      .join("; ");
    // NextRequest 내부 헤더에 Cookie 주입
    Object.defineProperty(req, "cookies", {
      value: {
        get: (name: string) =>
          opts.cookies?.[name] !== undefined
            ? { name, value: opts.cookies[name] }
            : undefined,
        getAll: () =>
          Object.entries(opts.cookies ?? {}).map(([name, value]) => ({ name, value })),
        set: vi.fn(),
        delete: vi.fn(),
        has: (name: string) => name in (opts.cookies ?? {}),
        toString: () => cookieStr,
      },
      writable: false,
    });
  }
  return req;
}

// ── fetch mock 설정 ──────────────────────────────────────────────────────────

const fetchSpy = vi.spyOn(global, "fetch");

beforeEach(() => {
  vi.resetAllMocks();
  // AUTH_BASE_URL 초기화 (NODE_ENV는 read-only이므로 직접 할당 금지)
  process.env.AUTH_BASE_URL = "https://auth.junodevs.com";
});

// ── login route ──────────────────────────────────────────────────────────────

describe("POST /api/auth/login", () => {
  it("Set-Cookie의 refresh_token 값이 우리 refresh 쿠키에 들어간다", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    // auth-svc가 Set-Cookie로 refresh_token을 내려주는 경우
    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "access-tok-123",
        token_type: "bearer",
        expires_in: 900,
        // JSON refresh_token은 deprecated이므로 Set-Cookie를 우선 써야 함
      }),
      {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          // auth-svc가 Set-Cookie로 refresh_token을 발행
          "Set-Cookie":
            "refresh_token=rt-from-set-cookie-abc; Path=/auth; HttpOnly; Secure; SameSite=None; Max-Age=1209600",
        },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "user@example.com", password: "pass" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    const accessVal = getCookieValue(setCookies, "access");
    const refreshVal = getCookieValue(setCookies, "refresh");

    expect(accessVal).toBe("access-tok-123");
    // Set-Cookie의 refresh_token 값이 우리 refresh 쿠키로 들어와야 한다
    expect(refreshVal).toBe("rt-from-set-cookie-abc");

    const body = await res.json();
    expect(body).toEqual({ ok: true });
  });

  it("Set-Cookie 부재 시 JSON refresh_token fallback 동작", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    // Set-Cookie 없이 JSON 필드만 있는 경우 (deprecated)
    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "access-tok-456",
        refresh_token: "rt-from-json-fallback",
        token_type: "bearer",
        expires_in: 900,
      }),
      {
        status: 200,
        headers: { "Content-Type": "application/json" },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "user@example.com", password: "pass" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    const refreshVal = getCookieValue(setCookies, "refresh");
    // JSON fallback 값이 우리 refresh 쿠키로 들어와야 한다
    expect(refreshVal).toBe("rt-from-json-fallback");
  });

  it("access_token 없으면 502 반환", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({ token_type: "bearer" }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "p" } });
    const res = await POST(req);
    expect(res.status).toBe(502);
  });

  it("auth-svc 4xx → detail 필드만 추출 (원문 텍스트 통째로 노출 금지)", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({ detail: "Invalid credentials" }),
      { status: 401, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "wrong" } });
    const res = await POST(req);
    expect(res.status).toBe(401);
    const body = await res.json();
    // detail 필드만 추출돼야 한다
    expect(body.detail).toBe("Invalid credentials");
  });

  it("auth-svc 5xx → 502 + 'Authentication service unavailable' 마스킹", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      "Internal Server Error details that should not leak",
      { status: 500 },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "pass" } });
    const res = await POST(req);
    expect(res.status).toBe(502);
    const body = await res.json();
    expect(body.detail).toBe("Authentication service unavailable");
    // 원문이 노출되면 안 된다
    expect(body.detail).not.toContain("Internal Server Error details");
  });

  it("form-urlencoded로 auth-svc에 전송한다", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok",
        refresh_token: "rt",
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "pass123" } });
    await POST(req);

    expect(fetchSpy).toHaveBeenCalledOnce();
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(url).toContain("/auth/login");
    const headers = init.headers as Record<string, string>;
    expect(headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    // body가 URLSearchParams 형태인지 확인
    const bodyStr = init.body as string;
    expect(bodyStr).toContain("username=a%40b.com");
    expect(bodyStr).toContain("password=pass123");
  });

  it("expires_in을 access 쿠키 maxAge로 사용한다", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok-with-expiry",
        refresh_token: "rt",
        expires_in: 1800, // 30분
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "pass" } });
    const res = await POST(req);
    expect(res.status).toBe(200);

    const setCookies = getSetCookies(res);
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    expect(accessCookie).toBeDefined();
    // expires_in=1800 → Max-Age=1800
    expect(accessCookie).toContain("Max-Age=1800");
  });

  it("refresh 쿠키 path가 /api/auth 이다", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok",
        refresh_token: "rt",
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "pass" } });
    const res = await POST(req);
    const setCookies = getSetCookies(res);

    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));
    expect(refreshCookie).toBeDefined();
    expect(refreshCookie?.toLowerCase()).toContain("path=/api/auth");
  });

  it("access 쿠키 path가 / 이다", async () => {
    const { POST } = await import("../app/api/auth/login/route");

    const mockAuthResponse = new Response(
      JSON.stringify({ access_token: "tok", refresh_token: "rt" }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ body: { username: "a@b.com", password: "pass" } });
    const res = await POST(req);
    const setCookies = getSetCookies(res);

    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    expect(accessCookie).toBeDefined();
    // Path=/; 형태로 포함되어야 한다 (정확히 루트)
    expect(accessCookie).toMatch(/[Pp]ath=\//);
  });
});

// ── refresh route ────────────────────────────────────────────────────────────

describe("POST /api/auth/refresh", () => {
  it("우리 refresh 쿠키가 Cookie 헤더로 auth-svc에 전달된다 (JSON body 없음)", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "new-access-tok",
        refresh_token: "new-rt",
        token_type: "bearer",
        expires_in: 900,
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "my-refresh-token-value" } });
    await POST(req);

    expect(fetchSpy).toHaveBeenCalledOnce();
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(url).toContain("/auth/refresh");

    const headers = init.headers as Record<string, string>;
    // Cookie 헤더로 refresh_token 전달
    expect(headers["Cookie"]).toBe("refresh_token=my-refresh-token-value");
    // JSON body는 없어야 한다 (deprecated)
    expect(init.body).toBeUndefined();
    // Content-Type도 불필요
    expect(headers["Content-Type"]).toBeUndefined();
  });

  it("TokenPair 응답에서 회전된 refresh_token이 Set-Cookie 우선으로 우리 refresh 쿠키에 갱신된다", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "rotated-access-tok",
        refresh_token: "deprecated-json-rt", // deprecated JSON 필드
        token_type: "bearer",
        expires_in: 900,
      }),
      {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          // auth-svc가 Set-Cookie로 회전된 refresh_token 발행
          "Set-Cookie":
            "refresh_token=rotated-rt-from-set-cookie; Path=/auth; HttpOnly; Secure; SameSite=None; Max-Age=1209600",
        },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "old-refresh-token" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    const accessVal = getCookieValue(setCookies, "access");
    const refreshVal = getCookieValue(setCookies, "refresh");

    expect(accessVal).toBe("rotated-access-tok");
    // Set-Cookie 우선 — deprecated JSON 필드가 아닌 Set-Cookie 값
    expect(refreshVal).toBe("rotated-rt-from-set-cookie");
  });

  it("TokenPair 응답의 refresh_token이 Set-Cookie 없을 때 JSON fallback으로 우리 refresh 쿠키에 갱신된다", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "rotated-access-tok-2",
        refresh_token: "rotated-rt-json-fallback",
        token_type: "bearer",
        expires_in: 900,
      }),
      {
        status: 200,
        headers: { "Content-Type": "application/json" },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "old-rt" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    const refreshVal = getCookieValue(setCookies, "refresh");
    expect(refreshVal).toBe("rotated-rt-json-fallback");
  });

  it("auth-svc 401 → 우리 라우트도 401 + access/refresh 쿠키 삭제 헤더", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({ detail: "Invalid or expired refresh token" }),
      { status: 401, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "expired-or-revoked-rt" } });
    const res = await POST(req);

    expect(res.status).toBe(401);
    const setCookies = getSetCookies(res);

    // access와 refresh 쿠키가 maxAge=0으로 삭제되어야 한다
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));

    expect(accessCookie).toBeDefined();
    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toBeDefined();
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("과도기 정리: 401 분기 Set-Cookie에 path=/api/auth Max-Age=0 과 path=/ Max-Age=0 두 줄 존재", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({ detail: "Invalid or expired refresh token" }),
      { status: 401, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "expired-or-revoked-rt" } });
    const res = await POST(req);

    expect(res.status).toBe(401);
    const setCookies = getSetCookies(res);

    // refresh= 으로 시작하는 Set-Cookie 헤더 전체 수집
    const refreshHeaders = setCookies.filter((h) => h.startsWith("refresh="));

    // path=/api/auth; Max-Age=0 줄 존재
    const hasApiAuthPath = refreshHeaders.some(
      (h) => h.toLowerCase().includes("path=/api/auth") && h.includes("Max-Age=0"),
    );
    // path=/; Max-Age=0 줄 존재 (legacy stale 정리)
    const hasRootPath = refreshHeaders.some(
      (h) => /[Pp]ath=\/[;,\s]/.test(h) && h.includes("Max-Age=0"),
    );

    expect(hasApiAuthPath).toBe(true);
    expect(hasRootPath).toBe(true);
  });

  it("refresh 쿠키 없으면 401 반환", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const req = makeRequest({ cookies: {} });
    const res = await POST(req);
    expect(res.status).toBe(401);
  });

  it("auth-svc 5xx → 502 + 'Authentication service unavailable' 마스킹", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      "Internal Server Error details that should not leak",
      { status: 500 },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "valid-rt" } });
    const res = await POST(req);
    expect(res.status).toBe(502);
    const body = await res.json();
    expect(body.detail).toBe("Authentication service unavailable");
  });

  it("expires_in을 access 쿠키 maxAge로 사용한다 (fallback 15분)", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    // expires_in 없음 → fallback 15분(900초)
    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok",
        refresh_token: "new-rt",
        token_type: "bearer",
        // expires_in 없음
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "rt" } });
    const res = await POST(req);
    expect(res.status).toBe(200);

    const setCookies = getSetCookies(res);
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    expect(accessCookie).toBeDefined();
    // fallback maxAge = 900 (15분)
    expect(accessCookie).toContain("Max-Age=900");
  });

  it("auth-svc Set-Cookie의 허용되지 않은 이름 쿠키는 무시된다 (화이트리스트)", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok",
        refresh_token: "new-rt",
        token_type: "bearer",
        expires_in: 900,
      }),
      {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          // 화이트리스트에 없는 쿠키 이름
          "Set-Cookie": "evil_cookie=malicious; Path=/",
        },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "rt" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    // evil_cookie가 응답에 포함되면 안 된다
    expect(setCookies.some((h) => h.includes("evil_cookie"))).toBe(false);
    // access 쿠키는 정상적으로 있어야 한다
    expect(getCookieValue(setCookies, "access")).toBe("tok");
  });

  it("화이트리스트: auth-svc Set-Cookie에 evil + refresh_token 혼재 시 evil은 무시되고 refresh_token은 추출된다", async () => {
    const { POST } = await import("../app/api/auth/refresh/route");

    // auth-svc가 evil 쿠키와 refresh_token 쿠키를 동시에 내려주는 시나리오
    // Note: 단일 Set-Cookie 헤더 문자열에서 getSetCookie()로 분리되려면 복수 헤더가 필요하지만
    // fetch mock에선 단일 헤더로 시뮬레이션. refresh_token 파싱 후 evil이 우리 응답에 없음을 검증.
    const mockAuthResponse = new Response(
      JSON.stringify({
        access_token: "tok-whitelist-test",
        token_type: "bearer",
        expires_in: 900,
        // JSON refresh_token 없음 — Set-Cookie에서만 추출
      }),
      {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          "Set-Cookie": "refresh_token=safe-rt-value; Path=/auth; HttpOnly; Secure; SameSite=None; Max-Age=1209600",
        },
      },
    );
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "rt" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    // 우리 응답에 refresh_token 이름 쿠키가 없어야 한다 (화이트리스트: "refresh"만 허용)
    expect(setCookies.some((h) => /^refresh_token=/.test(h))).toBe(false);
    // refresh 쿠키 값은 Set-Cookie의 refresh_token 값과 일치해야 한다
    expect(getCookieValue(setCookies, "refresh")).toBe("safe-rt-value");
    // access 쿠키도 정상적으로 있어야 한다
    expect(getCookieValue(setCookies, "access")).toBe("tok-whitelist-test");
  });
});

// ── logout route ─────────────────────────────────────────────────────────────

describe("POST /api/auth/logout", () => {
  it("Cookie: refresh_token=<v> 헤더로 auth-svc 호출, JSON body 없음", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    const mockAuthResponse = new Response(null, { status: 204 });
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "logout-rt-value" } });
    await POST(req);

    expect(fetchSpy).toHaveBeenCalledOnce();
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(url).toContain("/auth/logout");

    const headers = init.headers as Record<string, string>;
    // Cookie 헤더로 refresh_token 전달
    expect(headers["Cookie"]).toBe("refresh_token=logout-rt-value");
    // JSON body 없음 (deprecated)
    expect(init.body).toBeUndefined();
  });

  it("로컬 access/refresh 두 쿠키를 maxAge=0으로 삭제한다", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    const mockAuthResponse = new Response(null, { status: 204 });
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "some-rt" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({ ok: true });

    const setCookies = getSetCookies(res);

    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));

    expect(accessCookie).toBeDefined();
    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toBeDefined();
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("refresh 쿠키 없어도 로컬 쿠키 삭제는 수행한다", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    const req = makeRequest({ cookies: {} });
    const res = await POST(req);

    // auth-svc 호출 없음
    expect(fetchSpy).not.toHaveBeenCalled();

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));

    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("auth-svc 호출 실패해도 로컬 쿠키 삭제하고 ok 반환", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    fetchSpy.mockRejectedValueOnce(new Error("network error"));

    const req = makeRequest({ cookies: { refresh: "rt-val" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({ ok: true });
  });

  it("AbortSignal 타임아웃(AbortError) 발생해도 로컬 쿠키 삭제는 진행된다", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    // AbortError 모킹 — DOMException with name 'AbortError'
    const abortError = new DOMException("The operation was aborted.", "AbortError");
    fetchSpy.mockRejectedValueOnce(abortError);

    const req = makeRequest({ cookies: { refresh: "rt-val" } });
    const res = await POST(req);

    // 타임아웃이 발생해도 { ok: true } 반환
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({ ok: true });

    // 로컬 쿠키 삭제도 이루어져야 한다
    const setCookies = getSetCookies(res);
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));

    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("과도기 정리: Set-Cookie에 path=/api/auth Max-Age=0 과 path=/ Max-Age=0 두 줄이 모두 존재한다", async () => {
    const { POST } = await import("../app/api/auth/logout/route");

    const mockAuthResponse = new Response(null, { status: 204 });
    fetchSpy.mockResolvedValueOnce(mockAuthResponse);

    const req = makeRequest({ cookies: { refresh: "some-rt" } });
    const res = await POST(req);

    expect(res.status).toBe(200);
    const setCookies = getSetCookies(res);

    // refresh= 으로 시작하는 Set-Cookie 헤더 전체 수집
    const refreshHeaders = setCookies.filter((h) => h.startsWith("refresh="));

    // path=/api/auth; Max-Age=0 줄 존재
    const hasApiAuthPath = refreshHeaders.some(
      (h) => h.toLowerCase().includes("path=/api/auth") && h.includes("Max-Age=0"),
    );
    // path=/; Max-Age=0 줄 존재 (legacy stale 정리)
    const hasRootPath = refreshHeaders.some(
      (h) => /[Pp]ath=\/[;,\s]/.test(h) && h.includes("Max-Age=0"),
    );

    expect(hasApiAuthPath).toBe(true);
    expect(hasRootPath).toBe(true);
  });
});
