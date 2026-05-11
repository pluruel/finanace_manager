export type ClusterMemberView = {
  id: string;
  name: string;
  txn_count: number;
  latest_seen: string | null;
};

/** 트랜잭션 수 최댓값 멤버를 대표로. 동률 시 name 가나다순 첫 번째. */
export function pickDefaultCanonical(members: ClusterMemberView[]): string {
  const sorted = [...members].sort((a, b) => {
    if (b.txn_count !== a.txn_count) return b.txn_count - a.txn_count;
    return a.name.localeCompare(b.name, "ko");
  });
  return sorted[0]!.id;
}

/** 표시용 정렬: 트랜잭션 수 내림차순, 동률 시 가나다순. */
export function sortMembersForDisplay(members: ClusterMemberView[]): ClusterMemberView[] {
  return [...members].sort((a, b) => {
    if (b.txn_count !== a.txn_count) return b.txn_count - a.txn_count;
    return a.name.localeCompare(b.name, "ko");
  });
}

export function formatLatestSeen(date: string | null): string {
  return date ?? "—";
}
