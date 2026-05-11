"use client";

import { useState, useEffect, useCallback } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2 } from "lucide-react";
import { ProductListSchema, MerchantListSchema, type ProductItem, type MerchantItem } from "@/lib/schemas";

type ListItem = ProductItem | MerchantItem;

type Props = {
  scope: "product" | "merchant";
  onToast: (message: string, variant: "success" | "error") => void;
};

export function ManualMergePanel({ scope, onToast }: Props) {
  const [items, setItems] = useState<ListItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [canonicalId, setCanonicalId] = useState<string | null>(null);
  const [isMerging, setIsMerging] = useState(false);

  const fetchItems = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const proxyUrl =
        scope === "product" ? "/api/products-proxy" : "/api/merchants-proxy";
      const res = await fetch(proxyUrl, { cache: "no-store" });
      if (!res.ok) {
        setError("목록을 불러오지 못했습니다.");
        return;
      }
      const json: unknown = await res.json();
      if (scope === "product") {
        const parsed = ProductListSchema.safeParse(json);
        if (!parsed.success) {
          setError("응답 형식이 올바르지 않습니다.");
          return;
        }
        setItems(parsed.data);
      } else {
        const parsed = MerchantListSchema.safeParse(json);
        if (!parsed.success) {
          setError("응답 형식이 올바르지 않습니다.");
          return;
        }
        setItems(parsed.data);
      }
    } catch {
      setError("서버와 통신할 수 없습니다.");
    } finally {
      setLoading(false);
    }
  }, [scope]);

  // mount + scope change → fetch list, reset state
  useEffect(() => {
    setSelected(new Set());
    setCanonicalId(null);
    setQuery("");
    void fetchItems();
  }, [fetchItems]);

  const filtered = items.filter((item) =>
    item.name.toLowerCase().includes(query.toLowerCase()),
  );

  function toggleItem(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
        if (canonicalId === id) {
          setCanonicalId(null);
        }
      } else {
        next.add(id);
      }
      return next;
    });
  }

  const mergeDisabled = selected.size < 2 || canonicalId === null;

  let hintText = "";
  if (selected.size === 0) {
    hintText = "병합할 항목을 2개 이상 선택하세요";
  } else if (selected.size === 1) {
    hintText = "병합할 항목을 1개 더 선택하세요";
  } else if (canonicalId === null) {
    hintText = "대표 항목을 1개 선택하세요";
  }

  async function handleMerge() {
    if (mergeDisabled || !canonicalId) return;
    setIsMerging(true);
    try {
      const absorbIds = [...selected].filter((id) => id !== canonicalId);
      const res = await fetch("/api/clusters-proxy/merge", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          scope,
          canonical_id: canonicalId,
          absorb_ids: absorbIds,
        }),
      });
      if (!res.ok) {
        const json = await res.json().catch(() => ({}));
        const detail = (json as { detail?: string }).detail;
        onToast(detail ?? "병합에 실패했습니다.", "error");
        return;
      }
      onToast(`${selected.size}개 항목을 1개로 병합했습니다`, "success");
      setSelected(new Set());
      setCanonicalId(null);
      void fetchItems();
    } catch {
      onToast("서버와 통신할 수 없습니다.", "error");
    } finally {
      setIsMerging(false);
    }
  }

  return (
    <div className="space-y-3">
      <Input
        placeholder="이름으로 검색…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {loading && <p className="text-sm text-muted-foreground">불러오는 중…</p>}

      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <table className="w-full text-sm">
        <tbody>
          {filtered.map((item) => (
            <tr key={item.id} className="border-t">
              <td className="py-2 pr-2 w-8">
                <input
                  type="checkbox"
                  aria-label={`선택: ${item.name}`}
                  checked={selected.has(item.id)}
                  onChange={() => toggleItem(item.id)}
                />
              </td>
              <td className="py-2 font-medium">{item.name}</td>
              <td className="py-2 text-right text-muted-foreground">
                {scope === "product"
                  ? `거래 ${"transaction_count" in item ? item.transaction_count : 0}건`
                  : ""}
              </td>
            </tr>
          ))}
          {filtered.length === 0 && !loading && (
            <tr>
              <td colSpan={3} className="py-6 text-center text-muted-foreground">
                검색 결과가 없습니다
              </td>
            </tr>
          )}
        </tbody>
      </table>

      {selected.size > 0 && (
        <div className="border-t pt-3 space-y-2">
          <p className="text-sm font-medium">선택 {selected.size}개 — 대표 선택:</p>
          <div className="space-y-1 pl-2">
            {[...selected].map((id) => {
              const item = items.find((i) => i.id === id);
              if (!item) return null;
              return (
                <label key={id} className="flex items-center gap-2 text-sm">
                  <input
                    type="radio"
                    name="manual-canonical"
                    aria-label={`대표: ${item.name}`}
                    checked={canonicalId === id}
                    onChange={() => setCanonicalId(id)}
                  />
                  {item.name}
                </label>
              );
            })}
          </div>
          <p className="text-xs text-muted-foreground">{hintText}</p>
          <Button
            onClick={() => void handleMerge()}
            disabled={mergeDisabled || isMerging}
          >
            {isMerging ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                처리 중…
              </>
            ) : (
              `${selected.size}개 항목 병합`
            )}
          </Button>
        </div>
      )}
    </div>
  );
}
