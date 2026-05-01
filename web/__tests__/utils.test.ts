/**
 * 테스트 3: utils 함수 단위 테스트
 *
 * - formatAmount(음수, 0, 큰 값)
 * - formatKRW(null, 음수, 정수, 소수점)
 * - formatDate(빈 값, null, ISO 날짜)
 */

import { describe, it, expect } from "vitest";
import { formatAmount, formatKRW, formatDate } from "../lib/utils";

describe("formatAmount", () => {
  it("formats positive amount with sign=1", () => {
    const result = formatAmount("24900.00", 1);
    expect(result).toContain("24,900");
    expect(result).toContain("₩");
    expect(result).not.toMatch(/^-/);
  });

  it("formats negative amount with sign=-1", () => {
    const result = formatAmount("5000.00", -1);
    expect(result).toMatch(/^-/);
    expect(result).toContain("5,000");
  });

  it("formats zero amount", () => {
    const result = formatAmount("0", 1);
    expect(result).toContain("₩");
    // "₩0" 형태
    expect(result).toBe("₩0");
  });

  it("formats large amount", () => {
    const result = formatAmount("1000000.00", 1);
    expect(result).toContain("1,000,000");
    expect(result).not.toMatch(/^-/);
  });

  it("returns ₩0 for null", () => {
    const result = formatAmount(null, 1);
    expect(result).toBe("₩0");
  });

  it("returns ₩0 for undefined", () => {
    const result = formatAmount(undefined, 1);
    expect(result).toBe("₩0");
  });

  it("returns ₩0 for NaN-like string", () => {
    const result = formatAmount("invalid", 1);
    expect(result).toBe("₩0");
  });

  it("formats amount with sign=-1 as negative", () => {
    const result = formatAmount("7500.00", -1);
    expect(result.startsWith("-")).toBe(true);
    expect(result).toContain("7,500");
  });

  it("formats deduction amount (sign=1) without minus prefix", () => {
    // 차감 카테고리는 sign=+1이므로 음수 표시 없음
    const result = formatAmount("7500.00", 1);
    expect(result.startsWith("-")).toBe(false);
  });
});

describe("formatKRW", () => {
  it("formats a normal amount", () => {
    const result = formatKRW("15000.00");
    expect(result).toBe("₩15,000");
  });

  it("returns ₩0 for null", () => {
    expect(formatKRW(null)).toBe("₩0");
  });

  it("returns ₩0 for undefined", () => {
    expect(formatKRW(undefined)).toBe("₩0");
  });

  it("returns ₩0 for empty string", () => {
    expect(formatKRW("")).toBe("₩0");
  });

  it("handles negative amounts (abs value)", () => {
    // formatKRW는 절댓값을 사용
    const result = formatKRW("-5000.00");
    expect(result).toBe("₩5,000");
  });

  it("handles large amounts", () => {
    const result = formatKRW("1234567.00");
    expect(result).toBe("₩1,234,567");
  });

  it("handles decimal amount", () => {
    const result = formatKRW("1000.50");
    // toLocaleString이 소수점을 어떻게 처리하는지 확인 (환경에 따라 다름)
    expect(result).toContain("₩");
    expect(result).toContain("1,000");
  });
});

describe("formatDate", () => {
  it("returns ISO date string as-is", () => {
    expect(formatDate("2026-02-01")).toBe("2026-02-01");
  });

  it("returns empty string for null", () => {
    expect(formatDate(null)).toBe("");
  });

  it("returns empty string for undefined", () => {
    expect(formatDate(undefined)).toBe("");
  });

  it("preserves the date string without modification", () => {
    const date = "2026-12-31";
    expect(formatDate(date)).toBe(date);
  });

  it("handles any string value (pass-through)", () => {
    // formatDate는 단순 null coalescing이므로 어떤 문자열도 그대로 반환
    expect(formatDate("not-a-date")).toBe("not-a-date");
  });
});
