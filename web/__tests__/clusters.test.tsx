import { describe, it, expect } from "vitest";
import {
  pickDefaultCanonical,
  sortMembersForDisplay,
  formatLatestSeen,
} from "@/lib/cluster-data";

const m = (id: string, name: string, txn_count = 0, latest_seen: string | null = null) => ({
  id, name, txn_count, latest_seen,
});

describe("cluster-data helpers", () => {
  it("pickDefaultCanonical: 트랜잭션 수가 가장 많은 멤버를 고른다", () => {
    const members = [m("a", "A", 1), m("b", "B", 5), m("c", "C", 3)];
    expect(pickDefaultCanonical(members)).toBe("b");
  });

  it("pickDefaultCanonical: 동률이면 가나다순 첫 번째", () => {
    const members = [m("b", "Bravo", 3), m("a", "Alpha", 3)];
    expect(pickDefaultCanonical(members)).toBe("a");
  });

  it("sortMembersForDisplay: 트랜잭션 수 내림차순", () => {
    const members = [m("a", "A", 1), m("b", "B", 5), m("c", "C", 3)];
    const sorted = sortMembersForDisplay(members);
    expect(sorted.map(s => s.id)).toEqual(["b", "c", "a"]);
  });

  it("formatLatestSeen: 날짜 문자열 그대로, null 은 dash", () => {
    expect(formatLatestSeen("2026-02-28")).toBe("2026-02-28");
    expect(formatLatestSeen(null)).toBe("—");
  });
});
