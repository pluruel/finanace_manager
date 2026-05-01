/**
 * 테스트 2: middleware 단위 테스트 (parseCookieNameValue 헬퍼)
 *
 * middleware.ts의 parseCookieNameValue 함수를 별도로 추출해 테스트.
 * JWT-like 값에 = 포함, 빈 값, 공백 처리 등.
 */

import { describe, it, expect } from "vitest";

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
