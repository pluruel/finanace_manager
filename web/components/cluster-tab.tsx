"use client";

import { useState, useTransition } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { ClusterCard } from "@/components/cluster-card";
import { ClustersResponseSchema, type Cluster } from "@/lib/schemas";

export function ClusterTab() {
  const [scope, setScope] = useState<"product" | "merchant">("product");
  const [threshold, setThreshold] = useState<number>(0.5);
  const [clusters, setClusters] = useState<Cluster[] | null>(null);
  const [truncated, setTruncated] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  async function recompute() {
    setError(null);
    try {
      const res = await fetch(
        `/api/clusters-proxy?scope=${encodeURIComponent(scope)}&threshold=${encodeURIComponent(threshold)}`,
        { cache: "no-store" }
      );
      const json: unknown = await res.json();
      const parsed = ClustersResponseSchema.safeParse(json);
      if (!parsed.success) {
        setError("응답 형식이 올바르지 않습니다.");
        return;
      }
      setClusters(parsed.data.clusters);
      setTruncated(parsed.data.truncated);
    } catch {
      setError("서버와 통신할 수 없습니다.");
    }
  }

  async function merge(canonicalId: string, absorbIds: string[]) {
    setError(null);
    try {
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
        setError((json as { detail?: string }).detail ?? "병합에 실패했습니다.");
        return;
      }
    } catch {
      setError("서버와 통신할 수 없습니다.");
      return;
    }
    startTransition(() => {
      recompute();
    });
  }

  return (
    <div className="space-y-4">
      <Tabs
        value={scope}
        onValueChange={(v) => setScope(v as "product" | "merchant")}
      >
        <TabsList>
          <TabsTrigger value="product">상품</TabsTrigger>
          <TabsTrigger value="merchant">가맹점</TabsTrigger>
        </TabsList>
      </Tabs>

      <div className="flex items-center gap-3">
        <label className="text-sm text-muted-foreground">
          유사도 임계치: {threshold.toFixed(2)}
        </label>
        <input
          type="range"
          min={0.3}
          max={0.9}
          step={0.05}
          value={threshold}
          onChange={(e) => setThreshold(parseFloat(e.target.value))}
          className="w-40"
        />
      </div>

      <Button onClick={() => recompute()} disabled={isPending}>
        다시 계산
      </Button>

      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {truncated && (
        <Alert>
          <AlertDescription>
            결과가 너무 많아 일부만 표시됩니다. 임계치를 높이면 더 정확한 결과를 얻을 수 있습니다.
          </AlertDescription>
        </Alert>
      )}

      {clusters !== null && clusters.length === 0 && (
        <p className="text-sm text-muted-foreground">묶을 후보가 없습니다</p>
      )}

      {clusters !== null &&
        clusters.map((c, idx) => (
          <ClusterCard
            key={`${c.suggested_canonical_id}-${idx}`}
            cluster={c}
            onMerge={merge}
          />
        ))}
    </div>
  );
}
