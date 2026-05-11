"use client";

import { useEffect, useMemo, useState } from "react";
import { Loader2 } from "lucide-react";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  pickDefaultCanonical,
  sortMembersForDisplay,
  formatLatestSeen,
} from "@/lib/cluster-data";
import type { Cluster } from "@/lib/schemas";

type Props = {
  cluster: Cluster;
  onMerge: (canonicalId: string, absorbIds: string[]) => Promise<void>;
};

export function ClusterCard({ cluster, onMerge }: Props) {
  const sorted = useMemo(() => sortMembersForDisplay(cluster.members), [cluster.members]);
  const [canonicalId, setCanonicalId] = useState<string>(
    cluster.suggested_canonical_id || pickDefaultCanonical(cluster.members)
  );
  const [absorb, setAbsorb] = useState<Set<string>>(
    () => new Set(sorted.filter(m => m.id !== cluster.suggested_canonical_id).map(m => m.id))
  );
  const [isMerging, setIsMerging] = useState(false);

  // C1: cluster prop 변경 시 내부 상태 리셋
  useEffect(() => {
    const defaultCanonical = cluster.suggested_canonical_id || pickDefaultCanonical(cluster.members);
    setCanonicalId(defaultCanonical);
    setAbsorb(new Set(cluster.members.filter(m => m.id !== defaultCanonical).map(m => m.id)));
  }, [cluster]);

  // B1: canonical swap — 이전 canonical은 흡수로 자동 이동
  const onPickCanonical = (id: string) => {
    setCanonicalId((prevId) => {
      if (prevId !== id) {
        setAbsorb((prev) => {
          const next = new Set(prev);
          next.delete(id);    // 새 canonical은 흡수에서 제외
          next.add(prevId);   // 이전 canonical은 흡수로 자동 이동 (swap)
          return next;
        });
      }
      return id;
    });
  };

  const toggleAbsorb = (id: string) => {
    setAbsorb(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const absorbList = [...absorb];
  const mergeDisabled = absorbList.length === 0;

  // C2: merge in-flight disabled
  const handleMerge = async () => {
    setIsMerging(true);
    try {
      await onMerge(canonicalId, absorbList);
    } finally {
      setIsMerging(false);
    }
  };

  // D2: 거래 합계
  const totalTxns = sorted.reduce((s, m) => s + m.txn_count, 0);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {sorted.length}개 후보 · 평균 유사도 {(cluster.avg_similarity * 100).toFixed(0)}% · 거래 {totalTxns}건
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <table className="w-full text-sm">
          <tbody>
            {sorted.map(member => {
              const isCanonical = member.id === canonicalId;
              return (
                <tr key={member.id} className="border-t">
                  <td className="px-3 py-2">
                    <input
                      type="radio"
                      name={`canonical-${cluster.suggested_canonical_id}`}
                      aria-label={`대표: ${member.name}`}
                      checked={isCanonical}
                      disabled={isMerging}
                      onChange={() => onPickCanonical(member.id)}
                    />
                  </td>
                  <td className="px-3 py-2">
                    <input
                      type="checkbox"
                      aria-label={`흡수: ${member.name}`}
                      checked={absorb.has(member.id)}
                      disabled={isCanonical || isMerging}
                      onChange={() => toggleAbsorb(member.id)}
                    />
                  </td>
                  <td className="px-3 py-2 font-medium">{member.name}</td>
                  <td className="px-3 py-2 text-right text-muted-foreground">
                    거래 {member.txn_count}건
                  </td>
                  <td className="px-3 py-2 text-right text-muted-foreground">
                    최근 {formatLatestSeen(member.latest_seen)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </CardContent>
      <CardFooter className="justify-end items-center">
        {/* D1: 흡수 0개 hint */}
        {mergeDisabled && (
          <p className="text-xs text-muted-foreground mr-3">
            최소 1개 이상 흡수할 항목을 선택하세요
          </p>
        )}
        <Button
          disabled={mergeDisabled || isMerging}
          onClick={() => void handleMerge()}
        >
          {isMerging ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin mr-1" />
              처리 중...
            </>
          ) : (
            "병합"
          )}
        </Button>
      </CardFooter>
    </Card>
  );
}
