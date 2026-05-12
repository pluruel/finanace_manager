"use client";

import { useState, useCallback } from "react";
import { ClusterRecommendPanel } from "@/components/cluster-recommend-panel";
import { ManualMergePanel } from "@/components/manual-merge-panel";

// ── 모드 토글 버튼 그룹 (추천 / 수동) ──────────────────────────────────────
// Radix Tabs는 jsdom 환경에서 onValueChange가 fireEvent.click으로 트리거되지 않아
// 테스트 신뢰성을 위해 plain button 그룹으로 구현한다.
function ModeToggle({
  mode,
  onChange,
}: {
  mode: "recommend" | "manual";
  onChange: (m: "recommend" | "manual") => void;
}) {
  const base =
    "inline-flex items-center justify-center whitespace-nowrap rounded-sm px-3 py-1.5 text-sm font-medium ring-offset-background transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2";
  const active = "bg-background text-foreground shadow-sm";
  const inactive = "text-muted-foreground";
  return (
    <div
      role="tablist"
      aria-label="병합 모드"
      className="inline-flex h-10 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground"
    >
      <button
        role="tab"
        aria-selected={mode === "recommend"}
        className={`${base} ${mode === "recommend" ? active : inactive}`}
        onClick={() => onChange("recommend")}
        type="button"
      >
        추천
      </button>
      <button
        role="tab"
        aria-selected={mode === "manual"}
        className={`${base} ${mode === "manual" ? active : inactive}`}
        onClick={() => onChange("manual")}
        type="button"
      >
        수동
      </button>
    </div>
  );
}

// ── 스코프 토글 버튼 그룹 (상품 / 가맹점 / 카테고리) ────────────────────────
// 동일한 이유로 plain button으로 구현한다.
type Scope = "product" | "merchant" | "category";

function ScopeToggle({
  scope,
  onChange,
}: {
  scope: Scope;
  onChange: (s: Scope) => void;
}) {
  const base =
    "inline-flex items-center justify-center whitespace-nowrap rounded-sm px-3 py-1.5 text-sm font-medium ring-offset-background transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2";
  const active = "bg-background text-foreground shadow-sm";
  const inactive = "text-muted-foreground";
  const scopes: { value: Scope; label: string }[] = [
    { value: "product", label: "상품" },
    { value: "merchant", label: "가맹점" },
    { value: "category", label: "카테고리" },
  ];
  return (
    <div
      role="tablist"
      aria-label="스코프"
      className="inline-flex h-10 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground"
    >
      {scopes.map(({ value, label }) => (
        <button
          key={value}
          role="tab"
          aria-selected={scope === value}
          className={`${base} ${scope === value ? active : inactive}`}
          onClick={() => onChange(value)}
          type="button"
        >
          {label}
        </button>
      ))}
    </div>
  );
}

// ── Toast ─────────────────────────────────────────────────────────────────────

type ToastMessage = {
  id: number;
  message: string;
  variant: "success" | "error";
};

let toastCounter = 0;

function useToast() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const show = useCallback((message: string, variant: "success" | "error" = "success") => {
    const id = ++toastCounter;
    setToasts((prev) => [...prev, { id, message, variant }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  }, []);

  return { toasts, show };
}

function Toaster({ toasts }: { toasts: ToastMessage[] }) {
  if (toasts.length === 0) return null;
  return (
    <div
      className="fixed bottom-4 right-4 z-50 flex flex-col gap-2"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`rounded-md border px-4 py-3 text-sm shadow-md max-w-sm ${
            t.variant === "success"
              ? "bg-green-50 border-green-200 text-green-800"
              : "bg-destructive/10 border-destructive/30 text-destructive"
          }`}
        >
          {t.message}
        </div>
      ))}
    </div>
  );
}

// ── ClusterTab ─────────────────────────────────────────────────────────────────

export function ClusterTab() {
  const [mode, setMode] = useState<"recommend" | "manual">("recommend");
  const [scope, setScope] = useState<Scope>("product");
  const { toasts, show } = useToast();

  return (
    <div className="space-y-4">
      {/* 추천 / 수동 모드 토글 */}
      <ModeToggle mode={mode} onChange={setMode} />

      {/* 상품 / 가맹점 / 카테고리 스코프 (두 모드 공유) */}
      <ScopeToggle scope={scope} onChange={setScope} />

      {mode === "recommend" ? (
        <ClusterRecommendPanel scope={scope} onToast={show} />
      ) : (
        <ManualMergePanel scope={scope} onToast={show} />
      )}

      <Toaster toasts={toasts} />
    </div>
  );
}
