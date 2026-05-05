/**
 * middleware.test.ts
 *
 * middleware.ts 단위 테스트.
 * performRefresh를 vi.mock으로 모킹해 self-fetch 없는 구조를 검증한다.
 *
 * 검증 항목:
 *   - access 쿠키 있으면 NextResponse.next() 통과
 *   - access 없고 refresh 없으면 /login?from=... redirect + access/refresh 쿠키 삭제 헤더
 *   - access 없고 performRefresh ok:true → NextResponse.next에 access/refresh Set-Cookie
 *   - performRefresh ok:false 401 → redirect + 두 쿠키 삭제 + from 파라미터
 *   - 공개 경로(/login, /api/auth/*) 는 performRefresh 호출 없이 통과
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { NextRequest } from "next/server";
import type { RefreshResult } from "../lib/perform-refresh";

// ── performRefresh mock ────────────────────────────────────────────────────────

// middleware가 @/lib/perform-refresh 에서 import하므로 해당 모듈을 mock한다
vi.mock("../lib/perform-refresh", () => ({
  performRefresh: vi.fn(),
}));

// ── helpers ──────────────────────────────────────────────────────────────────

/** NextRequest 생성 헬퍼 */
function makeRequest(
  pathname: string,
  cookies: Record<string, string> = {},
): NextRequest {
  const url = `http://localhost:3000${pathname}`;
  const req = new NextRequest(url);

  Object.defineProperty(req, "cookies", {
    value: {
      get: (name: string) =>
        cookies[name] !== undefined ? { name, value: cookies[name] } : undefined,
      getAll: () =>
        Object.entries(cookies).map(([name, value]) => ({ name, value })),
      set: vi.fn(),
      delete: vi.fn(),
      has: (name: string) => name in cookies,
    },
    writable: false,
  });

  return req;
}

/** Response에서 Set-Cookie 헤더 배열 반환 */
function getSetCookies(response: Response): string[] {
  if (typeof (response.headers as Headers & { getSetCookie?: () => string[] }).getSetCookie === "function") {
    return (response.headers as Headers & { getSetCookie: () => string[] }).getSetCookie();
  }
  const raw = response.headers.get("set-cookie");
  return raw ? [raw] : [];
}

// ── 테스트 ─────────────────────────────────────────────────────────────────────

describe("middleware", () => {
  let performRefresh: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    vi.resetModules();
    vi.resetAllMocks();
    const mod = await import("../lib/perform-refresh");
    performRefresh = mod.performRefresh as ReturnType<typeof vi.fn>;
  });

  it("access 쿠키 있으면 performRefresh 호출 없이 통과", async () => {
    const { middleware } = await import("../middleware");

    const req = makeRequest("/dashboard", { access: "valid-access-token" });
    const res = await middleware(req);

    // redirect가 아닌 통과 응답
    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);
    // performRefresh 호출되지 않아야 한다
    expect(performRefresh).not.toHaveBeenCalled();
  });

  it("access 없고 refresh도 없으면 /login?from=... redirect + 쿠키 삭제", async () => {
    const { middleware } = await import("../middleware");

    // performRefresh를 ok:false 반환하도록 설정 (refresh=undefined로 호출됨)
    performRefresh.mockResolvedValueOnce({ ok: false, status: 401 } satisfies RefreshResult);

    const req = makeRequest("/dashboard", {});
    const res = await middleware(req);

    // redirect 응답
    expect(res.status).toBe(307);
    const location = res.headers.get("location");
    expect(location).toContain("/login");
    expect(location).toContain("from=");
    expect(location).toContain("%2Fdashboard");

    // access/refresh 쿠키 삭제 헤더 포함
    const setCookies = getSetCookies(res);
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));
    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("performRefresh ok:true → NextResponse.next에 access/refresh Set-Cookie", async () => {
    const { middleware } = await import("../middleware");

    const successResult: RefreshResult = {
      ok: true,
      setCookies: [
        { name: "access", value: "new-access-token", maxAge: 900 },
        { name: "refresh", value: "new-refresh-token", maxAge: 1209600 },
      ],
    };
    performRefresh.mockResolvedValueOnce(successResult);

    const req = makeRequest("/dashboard", { refresh: "old-refresh-token" });
    const res = await middleware(req);

    // redirect가 아닌 통과 응답
    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);

    // 새 쿠키 Set-Cookie 헤더 포함
    const setCookies = getSetCookies(res);
    const accessCookieHeader = setCookies.find((h) => h.startsWith("access="));
    const refreshCookieHeader = setCookies.find((h) => h.startsWith("refresh="));

    expect(accessCookieHeader).toBeDefined();
    expect(accessCookieHeader).toContain("new-access-token");
    expect(refreshCookieHeader).toBeDefined();
    expect(refreshCookieHeader).toContain("new-refresh-token");
  });

  it("performRefresh ok:false 401 → redirect + 두 쿠키 삭제 + from 파라미터", async () => {
    const { middleware } = await import("../middleware");

    performRefresh.mockResolvedValueOnce({ ok: false, status: 401 } satisfies RefreshResult);

    const req = makeRequest("/transactions", { refresh: "revoked-token" });
    const res = await middleware(req);

    expect(res.status).toBe(307);
    const location = res.headers.get("location");
    expect(location).toContain("/login");
    expect(location).toContain("from=");
    // from 파라미터에 원래 경로가 인코딩되어 있어야 한다
    expect(location).toContain("%2Ftransactions");

    const setCookies = getSetCookies(res);
    const accessCookie = setCookies.find((h) => h.startsWith("access="));
    const refreshCookie = setCookies.find((h) => h.startsWith("refresh="));
    expect(accessCookie).toContain("Max-Age=0");
    expect(refreshCookie).toContain("Max-Age=0");
  });

  it("과도기 정리: ok:false 401 redirect에 refresh path=/api/auth Max-Age=0 과 path=/ Max-Age=0 두 줄 존재", async () => {
    const { middleware } = await import("../middleware");

    performRefresh.mockResolvedValueOnce({ ok: false, status: 401 } satisfies RefreshResult);

    const req = makeRequest("/transactions", { refresh: "revoked-token" });
    const res = await middleware(req);

    expect(res.status).toBe(307);
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

  it("공개 경로 /login 은 performRefresh 호출 없이 통과", async () => {
    const { middleware } = await import("../middleware");

    const req = makeRequest("/login", {});
    const res = await middleware(req);

    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);
    expect(performRefresh).not.toHaveBeenCalled();
  });

  it("공개 경로 /api/auth/refresh 는 performRefresh 호출 없이 통과", async () => {
    const { middleware } = await import("../middleware");

    const req = makeRequest("/api/auth/refresh", {});
    const res = await middleware(req);

    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);
    expect(performRefresh).not.toHaveBeenCalled();
  });

  it("공개 경로 /api/auth/login 은 performRefresh 호출 없이 통과", async () => {
    const { middleware } = await import("../middleware");

    const req = makeRequest("/api/auth/login", {});
    const res = await middleware(req);

    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);
    expect(performRefresh).not.toHaveBeenCalled();
  });

  it("공개 경로 /api/auth/logout 은 performRefresh 호출 없이 통과", async () => {
    const { middleware } = await import("../middleware");

    const req = makeRequest("/api/auth/logout", {});
    const res = await middleware(req);

    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);
    expect(performRefresh).not.toHaveBeenCalled();
  });

  it("performRefresh ok:true 시 request.cookies.set이 새 access 토큰으로 호출된다", async () => {
    const { middleware } = await import("../middleware");

    const successResult: RefreshResult = {
      ok: true,
      setCookies: [
        { name: "access", value: "refreshed-access-token", maxAge: 900 },
        { name: "refresh", value: "refreshed-refresh-token", maxAge: 1209600 },
      ],
    };
    performRefresh.mockResolvedValueOnce(successResult);

    const cookiesMock = {
      get: vi.fn().mockImplementation((name: string) =>
        name === "refresh" ? { name, value: "old-refresh" } : undefined,
      ),
      getAll: vi.fn().mockReturnValue([{ name: "refresh", value: "old-refresh" }]),
      set: vi.fn(),
      delete: vi.fn(),
      has: vi.fn().mockImplementation((name: string) => name === "refresh"),
    };

    const url = "http://localhost:3000/dashboard";
    const req = new NextRequest(url);
    Object.defineProperty(req, "cookies", {
      value: cookiesMock,
      writable: false,
    });

    const res = await middleware(req);

    // redirect가 아닌 통과 응답
    expect(res.status).not.toBe(302);
    expect(res.status).not.toBe(307);

    // request.cookies.set이 새 access 토큰으로 호출되어야 한다 (RSC 전파)
    expect(cookiesMock.set).toHaveBeenCalledWith("access", "refreshed-access-token");
    expect(cookiesMock.set).toHaveBeenCalledWith("refresh", "refreshed-refresh-token");
  });

  it("performRefresh에 refresh 쿠키 값이 전달된다", async () => {
    const { middleware } = await import("../middleware");

    performRefresh.mockResolvedValueOnce({
      ok: true,
      setCookies: [{ name: "access", value: "new-tok", maxAge: 900 }],
    } satisfies RefreshResult);

    const req = makeRequest("/dashboard", { refresh: "my-refresh-token" });
    await middleware(req);

    expect(performRefresh).toHaveBeenCalledWith("my-refresh-token");
  });
});
