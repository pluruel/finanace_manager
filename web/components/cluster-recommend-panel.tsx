"use client";

import { useState, useCallback, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { ClusterCard } from "@/components/cluster-card";
import { ClustersResponseSchema, type Cluster } from "@/lib/schemas";

type Props = {
  scope: "product" | "merchant" | "category";
  onToast: (message: string, variant: "success" | "error") => void;
};

export function ClusterRecommendPanel({ scope, onToast }: Props) {
  const [threshold, setThreshold] = useState<number>(0.5);
  const [clusters, setClusters] = useState<Cluster[] | null>(null);
  const [truncated, setTruncated] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [isFetching, setIsFetching] = useState(false);

  const recompute = useCallback(async () => {
    setError(null);
    setIsFetching(true);
    try {
      const res = await fetch(
        `/api/clusters-proxy?scope=${encodeURIComponent(scope)}&threshold=${encodeURIComponent(threshold)}`,
        { cache: "no-store" },
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
    } finally {
      setIsFetching(false);
    }
  }, [scope, threshold]);

  // scope/threshold 변경 시 자동 디바운스 재계산 (mount 포함)
  useEffect(() => {
    setClusters(null);
    setTruncated(false);
    const t = setTimeout(() => { void recompute(); }, 300);
    return () => clearTimeout(t);
  }, [scope, threshold, recompute]);

  async function merge(canonicalId: string, absorbIds: string[]): Promise<void> {
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
        const detail = (json as { detail?: string }).detail;
        onToast(detail ?? "병합에 실패했습니다.", "error");
        return;
      }
    } catch {
      onToast("서버와 통신할 수 없습니다.", "error");
      return;
    }
    onToast(`${absorbIds.length}개 항목을 1개로 병합했습니다`, "success");
    void recompute();
  }

  return (
    <div className="space-y-4">
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

      <Button onClick={() => void recompute()} disabled={isFetching}>
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
        clusters.map((c) => (
          <ClusterCard
            key={c.members.map((m) => m.id).slice().sort().join("|")}
            cluster={c}
            onMerge={merge}
          />
        ))}
    </div>
  );
}
