"use client";

import { useRouter, usePathname, useSearchParams } from "next/navigation";
import { useTransition } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";

type Props = {
  year: number;
  month: number;
};

function shift(year: number, month: number, delta: number): { year: number; month: number } {
  const idx = year * 12 + (month - 1) + delta;
  const y = Math.floor(idx / 12);
  const m = (idx % 12 + 12) % 12;
  return { year: y, month: m + 1 };
}

function formatYM(year: number, month: number): string {
  return `${year}-${String(month).padStart(2, "0")}`;
}

export function MonthPicker({ year, month }: Props) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const [isPending, startTransition] = useTransition();

  const navigate = (y: number, m: number) => {
    const params = new URLSearchParams(searchParams.toString());
    params.set("ym", formatYM(y, m));
    startTransition(() => {
      router.push(`${pathname}?${params.toString()}`);
    });
  };

  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value; // "YYYY-MM"
    if (!/^\d{4}-\d{2}$/.test(v)) return;
    const [y, m] = v.split("-").map(Number);
    if (m < 1 || m > 12) return;
    navigate(y, m);
  };

  const prev = shift(year, month, -1);
  const next = shift(year, month, +1);

  return (
    <div className="flex items-center gap-2">
      <Button
        variant="outline"
        size="icon"
        aria-label="이전 달"
        disabled={isPending}
        onClick={() => navigate(prev.year, prev.month)}
      >
        <ChevronLeft className="h-4 w-4" />
      </Button>
      <input
        type="month"
        aria-label="월 선택"
        value={formatYM(year, month)}
        onChange={onChange}
        className="h-9 px-3 rounded-md border border-input bg-background text-sm tabular-nums"
      />
      <Button
        variant="outline"
        size="icon"
        aria-label="다음 달"
        disabled={isPending}
        onClick={() => navigate(next.year, next.month)}
      >
        <ChevronRight className="h-4 w-4" />
      </Button>
    </div>
  );
}
