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
 * sign과 amount를 고려해 표시 금액을 반환한다.
 * sign=-1이면 수입(마이너스 지출)이므로 앞에 - 표시.
 */
export function formatAmount(amount: string | null | undefined, sign: number): string {
  if (!amount) return "₩0";
  const num = parseFloat(amount);
  if (isNaN(num)) return "₩0";
  const formatted = `₩${num.toLocaleString("ko-KR")}`;
  return sign === -1 ? `-${formatted}` : formatted;
}

/**
 * "YYYY-MM-DD" 형식의 날짜를 그대로 반환한다.
 * null/undefined인 경우 빈 문자열을 반환한다.
 */
export function formatDate(dateStr: string | null | undefined): string {
  return dateStr ?? "";
}
