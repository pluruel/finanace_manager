import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * 금액(numeric string)을 한국 원화 형식으로 포맷한다.
 * 예: "15000.00" → "₩15,000"
 */
export function formatKRW(amount: string | null | undefined): string {
  if (!amount) return "₩0";
  const num = parseFloat(amount);
  if (isNaN(num)) return "₩0";
  return `₩${Math.abs(num).toLocaleString("ko-KR")}`;
}

/**
 * Decimal string 을 천단위 콤마 + 음수면 - 접두사로 표시.
 * 부호는 amount 문자열 자체에 들어 있다.
 */
export function formatAmount(amount: string | null | undefined): string {
  if (amount == null) return "";
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return amount;
  const formatted = Math.abs(v).toLocaleString();
  return v < 0 ? `-${formatted}` : formatted;
}

/**
 * "YYYY-MM-DD" 형식의 날짜를 그대로 반환한다.
 * null/undefined인 경우 빈 문자열을 반환한다.
 */
export function formatDate(dateStr: string | null | undefined): string {
  return dateStr ?? "";
}
