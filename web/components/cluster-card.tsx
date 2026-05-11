"use client";

import { useMemo, useState } from "react";
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
  onMerge: (canonicalId: string, absorbIds: string[]) => void;
};

export function ClusterCard({ cluster, onMerge }: Props) {
  const sorted = useMemo(() => sortMembersForDisplay(cluster.members), [cluster.members]);
  const [canonicalId, setCanonicalId] = useState<string>(
    cluster.suggested_canonical_id || pickDefaultCanonical(cluster.members)
  );
  const [absorb, setAbsorb] = useState<Set<string>>(
    () => new Set(sorted.filter(m => m.id !== cluster.suggested_canonical_id).map(m => m.id))
  );

  // canonical 변경 시 그 멤버는 흡수에서 제외
  const onPickCanonical = (id: string) => {
    setCanonicalId(id);
    setAbsorb(prev => {
      const next = new Set(prev);
      next.delete(id);
      return next;
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

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {sorted.length}개 후보 · 평균 유사도 {(cluster.avg_similarity * 100).toFixed(0)}%
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
                      onChange={() => onPickCanonical(member.id)}
                    />
                  </td>
                  <td className="px-3 py-2">
                    <input
                      type="checkbox"
                      aria-label={`흡수: ${member.name}`}
                      checked={absorb.has(member.id)}
                      disabled={isCanonical}
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
      <CardFooter className="justify-end">
        <Button
          disabled={mergeDisabled}
          onClick={() => onMerge(canonicalId, absorbList)}
        >
          병합
        </Button>
      </CardFooter>
    </Card>
  );
}
