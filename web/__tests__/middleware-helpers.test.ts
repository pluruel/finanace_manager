/**
 * 테스트 2: middleware 단위 테스트 (parseCookieNameValue 헬퍼)
 *
 * middleware.ts의 parseCookieNameValue 함수를 별도로 추출해 테스트.
 * JWT-like 값에 = 포함, 빈 값, 공백 처리 등.
 *
 * 하단 describe("lib/auth-cookies parseSetCookieNameValue") 는
 * lib/auth-cookies.ts에서 실제 export 된 함수를 직접 import해 검증한다.
 */

import { describe, it, expect } from "vitest";
import { parseSetCookieNameValue } from "../lib/auth-cookies";

// parseCookieNameValue는 middleware.ts에 non-exported 함수이므로
// 동일 로직을 여기서 재구현해 단위 테스트로 검증한다.
// (테스트 목적의 순수 로직 복제 — 파일 분리 없이)
function parseCookieNameValue(
  setCookieStr: string,
): { name: string; value: string } | null {
  const firstPart = setCookieStr.split(";")[0]?.trim();
  if (!firstPart) return null;
  const eqIdx = firstPart.indexOf("=");
  if (eqIdx === -1) return null;
  return {
    name: firstPart.slice(0, eqIdx).trim(),
    value: firstPart.slice(eqIdx + 1).trim(),
  };
}

describe("parseCookieNameValue", () => {
  it("parses a simple cookie", () => {
    const result = parseCookieNameValue("access=abc123");
    expect(result).toEqual({ name: "access", value: "abc123" });
  });

  it("parses a cookie with Set-Cookie attributes", () => {
    const result = parseCookieNameValue(
      "access=abc123; Path=/; HttpOnly; Max-Age=900",
    );
    expect(result).toEqual({ name: "access", value: "abc123" });
  });

  it("handles JWT-like value with = signs", () => {
    // JWT 토큰은 base64로 인코딩되어 = 패딩을 포함할 수 있다
    const jwtLike =
      "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0In0.signature==";
    const result = parseCookieNameValue(`access=${jwtLike}; Path=/; HttpOnly`);
    expect(result).not.toBeNull();
    expect(result?.name).toBe("access");
    // = 이후 첫 번째 부분만 잘리는 게 아니라 indexOf를 사용해 올바르게 처리됨
    expect(result?.value).toBe(jwtLike);
  });

  it("returns null for empty string", () => {
    const result = parseCookieNameValue("");
    expect(result).toBeNull();
  });

  it("returns null for whitespace only", () => {
    const result = parseCookieNameValue("   ");
    expect(result).toBeNull();
  });

  it("returns null when no = sign in name part", () => {
    const result = parseCookieNameValue("justname; Path=/");
    expect(result).toBeNull();
  });

  it("trims whitespace around name and value", () => {
    const result = parseCookieNameValue("  access  =  value123  ; Path=/");
    expect(result).toEqual({ name: "access", value: "value123" });
  });

  it("handles refresh token cookie", () => {
    const refreshToken = "some-opaque-refresh-token-value";
    const result = parseCookieNameValue(
      `refresh=${refreshToken}; Path=/; HttpOnly; Secure; SameSite=Strict`,
    );
    expect(result).toEqual({ name: "refresh", value: refreshToken });
  });

  it("handles empty value after =", () => {
    // 쿠키 값이 비어 있는 경우
    const result = parseCookieNameValue("access=; Path=/");
    expect(result).toEqual({ name: "access", value: "" });
  });

  it("handles multiple Set-Cookie parts correctly", () => {
    // 여러 ; 가 있는 경우도 첫 name=value만 추출
    const result = parseCookieNameValue(
      "access=tok; Path=/; HttpOnly; Secure; Max-Age=900; SameSite=Lax",
    );
    expect(result).toEqual({ name: "access", value: "tok" });
  });
});

// ── lib/auth-cookies의 실제 export 함수 단위 테스트 ────────────────────────────────

describe("lib/auth-cookies parseSetCookieNameValue", () => {
  it("빈 문자열 → null 반환", () => {
    expect(parseSetCookieNameValue("")).toBeNull();
  });

  it("이름만 있고 = 없음 → null 반환", () => {
    expect(parseSetCookieNameValue("onlyname")).toBeNull();
  });

  it("세미콜론 뒤 attribute만 있는 경우도 이름 없으면 null 반환", () => {
    // ';' 로만 이뤄진 경우 — firstPart가 빈 문자열이 됨
    expect(parseSetCookieNameValue("; Path=/")).toBeNull();
  });

  it("공백으로 둘러싸인 이름과 값을 trim해 파싱", () => {
    const result = parseSetCookieNameValue("  refresh  =  opaque-token-value  ; Path=/api/auth");
    expect(result).toEqual({ name: "refresh", value: "opaque-token-value" });
  });

  it("다중 attribute가 있는 Set-Cookie 헤더에서 name=value만 추출", () => {
    const result = parseSetCookieNameValue(
      "refresh_token=rt-abc; Path=/auth; HttpOnly; Secure; SameSite=None; Max-Age=1209600",
    );
    expect(result).toEqual({ name: "refresh_token", value: "rt-abc" });
  });

  it("= 기호를 포함하는 값(JWT base64 패딩)을 올바르게 파싱", () => {
    // JWT는 base64 패딩 '=' 를 포함할 수 있다
    const jwtLike = "eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ1c2VyIn0.sig==";
    const result = parseSetCookieNameValue(`access=${jwtLike}; Path=/; HttpOnly`);
    expect(result).not.toBeNull();
    expect(result?.name).toBe("access");
    expect(result?.value).toBe(jwtLike);
  });

  it("값이 비어있는 경우(삭제 쿠키) — name은 파싱되고 value는 빈 문자열", () => {
    const result = parseSetCookieNameValue("refresh=; Path=/api/auth; Max-Age=0");
    expect(result).toEqual({ name: "refresh", value: "" });
  });
});
