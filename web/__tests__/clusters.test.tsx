import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { vi } from "vitest";
import {
  pickDefaultCanonical,
  sortMembersForDisplay,
  formatLatestSeen,
} from "@/lib/cluster-data";
import { ClusterCard } from "@/components/cluster-card";

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

const sampleCluster = {
  members: [
    { id: "a", name: "고덕방 아이스아메리카노", txn_count: 6, latest_seen: "2026-02-28" },
    { id: "b", name: "고덕방 아메리카노",       txn_count: 2, latest_seen: "2026-02-15" },
    { id: "c", name: "고덕방 아아",             txn_count: 1, latest_seen: "2026-02-10" },
  ],
  suggested_canonical_id: "a",
  avg_similarity: 0.62,
};

describe("ClusterCard", () => {
  it("멤버를 트랜잭션 수 내림차순으로 렌더하고 최댓값을 라디오로 선택", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    const rows = screen.getAllByRole("row");
    // 행 첫 번째 = txn 6 (a)
    expect(rows[0].textContent).toContain("고덕방 아이스아메리카노");
    const radio = screen.getByLabelText("대표: 고덕방 아이스아메리카노") as HTMLInputElement;
    expect(radio.checked).toBe(true);
  });

  it("대표로 선택된 row 의 흡수 체크박스는 disabled", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    const cb = screen.getByLabelText("흡수: 고덕방 아이스아메리카노") as HTMLInputElement;
    expect(cb.disabled).toBe(true);
  });

  it("흡수 0개면 병합 버튼 disabled", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    // 기본은 나머지 흡수 ON. 두 흡수 체크 해제.
    fireEvent.click(screen.getByLabelText("흡수: 고덕방 아메리카노"));
    fireEvent.click(screen.getByLabelText("흡수: 고덕방 아아"));
    const btn = screen.getByRole("button", { name: /병합/ }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it("병합 버튼 클릭 시 onMerge(canonical_id, absorb_ids) 호출", () => {
    const onMerge = vi.fn();
    render(<ClusterCard cluster={sampleCluster} onMerge={onMerge} />);
    fireEvent.click(screen.getByRole("button", { name: /병합/ }));
    expect(onMerge).toHaveBeenCalledWith("a", expect.arrayContaining(["b", "c"]));
  });
});

import { ClusterTab } from "@/components/cluster-tab";

function mockFetchSequence(responses: Array<unknown>) {
  let i = 0;
  global.fetch = vi.fn(async () => {
    const body = responses[i++] ?? { clusters: [], scope: "product", threshold: 0.5, truncated: false };
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }) as unknown as typeof fetch;
}

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("ClusterTab", () => {
  it("초기에는 안내 텍스트만 보이고 fetch 안 함", () => {
    mockFetchSequence([]);
    render(<ClusterTab />);
    expect(screen.queryByText(/다시 계산/)).not.toBeNull();
    expect(global.fetch).not.toHaveBeenCalled();
  });

  it("'다시 계산' 클릭 시 fetch 후 카드 렌더", async () => {
    mockFetchSequence([{
      scope: "product", threshold: 0.5, truncated: false,
      clusters: [{
        members: [
          { id: "aaaaaaaa-0000-0000-0000-000000000001", name: "고덕방 아메리카노", txn_count: 3, latest_seen: "2026-02-28" },
          { id: "bbbbbbbb-0000-0000-0000-000000000002", name: "고덕방 아아",       txn_count: 1, latest_seen: "2026-02-15" },
        ],
        suggested_canonical_id: "aaaaaaaa-0000-0000-0000-000000000001",
        avg_similarity: 0.5,
      }],
    }]);
    render(<ClusterTab />);
    fireEvent.click(screen.getByRole("button", { name: /다시 계산/ }));
    expect(await screen.findByText("고덕방 아메리카노")).not.toBeNull();
  });
});
