"use client";

import { useState, useCallback, useMemo } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getExpandedRowModel,
  flexRender,
  createColumnHelper,
  ExpandedState,
  Row,
} from "@tanstack/react-table";
import { useRouter } from "next/navigation";
import { ChevronRight, ChevronDown } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { TransactionItem } from "@/lib/schemas";
import { cn, formatAmount, formatDate } from "@/lib/utils";

// ─── 행 타입 ──────────────────────────────────────────────────────────────
// react-table은 단일 행 타입을 쓴다.
// 백엔드 TransactionItem이 그대로 행 단위가 된다.
// children.length > 0 이면 multi-line 그룹 헤더.
type TableRow = {
  item: TransactionItem;
  subRows?: TableRow[];
};

/**
 * buildTableData
 *
 * 백엔드 응답 items를 react-table 행 트리로 변환한다.
 * - children.length > 0 인 item → 헤더 행 + 자식 행들
 * - children.length === 0 인 item → 단일 행 (subRows 없음)
 *
 * 백엔드는 이미 그룹핑을 완료해서 첫 번째 item 자체가 헤더이고
 * item.children이 나머지 자식 라인들이다.
 */
function buildTableData(items: TransactionItem[]): TableRow[] {
  return items.map((item): TableRow => {
    if (item.children.length > 0) {
      return {
        item,
        subRows: item.children.map((child): TableRow => ({ item: child })),
      };
    }
    return { item };
  });
}

// ─── 컬럼 정의 ────────────────────────────────────────────────────────────
const col = createColumnHelper<TableRow>();

const columns = [
  col.display({
    id: "expander",
    header: "",
    cell: ({ row }) => {
      if (!row.getCanExpand()) return null;
      return (
        <button
          onClick={row.getToggleExpandedHandler()}
          className="p-0.5 hover:bg-muted rounded transition-colors"
          aria-label={row.getIsExpanded() ? "접기" : "펼치기"}
          aria-expanded={row.getIsExpanded()}
        >
          {row.getIsExpanded() ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
        </button>
      );
    },
    size: 32,
  }),

  col.accessor((row) => row.item.occurred_on, {
    id: "occurred_on",
    header: "날짜",
    cell: ({ getValue }) => (
      <span className="text-sm tabular-nums">{formatDate(getValue<string>())}</span>
    ),
  }),

  col.accessor((row) => row.item.category_name ?? "", {
    id: "category",
    header: "카테고리",
    cell: ({ getValue }) => {
      const cat = getValue<string>();
      const isDeduction = cat === "차감";
      return (
        <div className="flex items-center gap-1.5">
          <span className="text-sm">{cat || "-"}</span>
          {isDeduction && (
            <Badge variant="muted" className="text-xs px-1.5 py-0">
              정산 차감
            </Badge>
          )}
        </div>
      );
    },
  }),

  col.accessor((row) => row.item.merchant_name ?? "", {
    id: "merchant",
    header: "구매처",
    cell: ({ getValue }) => (
      <span className="text-sm">{getValue<string>() || "-"}</span>
    ),
  }),

  col.accessor((row) => row.item.actor_name ?? "", {
    id: "actor",
    header: "액터",
    cell: ({ getValue }) => (
      <span className="text-sm">{getValue<string>() || "-"}</span>
    ),
  }),

  col.accessor((row) => row.item.product_name ?? "", {
    id: "product",
    header: "상품",
    cell: ({ getValue }) => (
      <span className="text-sm text-muted-foreground">
        {getValue<string>() || "-"}
      </span>
    ),
  }),

  col.accessor((row) => row.item.memo ?? "", {
    id: "memo",
    header: "메모",
    cell: ({ getValue }) => (
      <span className="text-sm text-muted-foreground">
        {getValue<string>() || "-"}
      </span>
    ),
  }),

  col.accessor((row) => row.item.unit_price, {
    id: "unit_price",
    header: "단가",
    cell: ({ getValue }) => {
      const v = getValue<string | null>();
      return (
        <span className="text-sm tabular-nums text-right">
          {v ? `₩${parseFloat(v).toLocaleString("ko-KR")}` : "-"}
        </span>
      );
    },
  }),

  col.accessor((row) => row.item.quantity, {
    id: "quantity",
    header: "수량",
    cell: ({ getValue }) => {
      const v = getValue<string | null>();
      return (
        <span className="text-sm tabular-nums text-right">
          {v ? parseFloat(v).toString() : "-"}
        </span>
      );
    },
  }),

  col.accessor(
    (row) => row.item.amount,
    {
      id: "amount",
      header: "금액",
      cell: ({ getValue }) => {
        const amount = getValue<string>();
        const isCashIn = parseFloat(amount) > 0;
        return (
          <span
            className={cn(
              "text-sm font-medium tabular-nums text-right",
              isCashIn ? "text-blue-600" : "text-foreground",
            )}
          >
            {formatAmount(amount)}
          </span>
        );
      },
    },
  ),

  col.accessor((row) => row.item.payment_method_name ?? "", {
    id: "payment_method",
    header: "결제수단",
    cell: ({ getValue }) => (
      <span className="text-sm">{getValue<string>() || "-"}</span>
    ),
  }),
];

// ─── 필터 바 ──────────────────────────────────────────────────────────────
function FilterBar({
  currentFilters,
  onFilterChange,
}: {
  currentFilters: Record<string, string>;
  onFilterChange: (key: string, value: string) => void;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      <Input
        placeholder="날짜 from (YYYY-MM-DD)"
        className="w-44 h-8 text-sm"
        defaultValue={currentFilters["from"] ?? ""}
        onBlur={(e) => onFilterChange("from", e.target.value)}
      />
      <Input
        placeholder="날짜 to (YYYY-MM-DD)"
        className="w-44 h-8 text-sm"
        defaultValue={currentFilters["to"] ?? ""}
        onBlur={(e) => onFilterChange("to", e.target.value)}
      />
      <Input
        placeholder="카테고리"
        className="w-32 h-8 text-sm"
        defaultValue={currentFilters["category"] ?? ""}
        onBlur={(e) => onFilterChange("category", e.target.value)}
      />
      <Input
        placeholder="구매처"
        className="w-32 h-8 text-sm"
        defaultValue={currentFilters["merchant"] ?? ""}
        onBlur={(e) => onFilterChange("merchant", e.target.value)}
      />
      <Input
        placeholder="액터"
        className="w-28 h-8 text-sm"
        defaultValue={currentFilters["actor"] ?? ""}
        onBlur={(e) => onFilterChange("actor", e.target.value)}
      />
      <Input
        placeholder="결제수단"
        className="w-32 h-8 text-sm"
        defaultValue={currentFilters["payment"] ?? ""}
        onBlur={(e) => onFilterChange("payment", e.target.value)}
      />
    </div>
  );
}

// ─── 메인 컴포넌트 ────────────────────────────────────────────────────────
interface TransactionsTableProps {
  items: TransactionItem[];
  total: number;
  searchParams: Record<string, string>;
}

export function TransactionsTable({
  items,
  total,
  searchParams,
}: TransactionsTableProps) {
  const router = useRouter();
  const [expanded, setExpanded] = useState<ExpandedState>({});

  const tableData = useMemo(() => buildTableData(items), [items]);

  const table = useReactTable({
    data: tableData,
    columns,
    state: { expanded },
    onExpandedChange: setExpanded,
    getSubRows: (row) => row.subRows,
    getCoreRowModel: getCoreRowModel(),
    getExpandedRowModel: getExpandedRowModel(),
    getRowCanExpand: (row) => (row.original.subRows?.length ?? 0) > 0,
  });

  const handleFilterChange = useCallback(
    (key: string, value: string) => {
      const params = new URLSearchParams(searchParams);
      if (value) {
        params.set(key, value);
      } else {
        params.delete(key);
      }
      router.push(`/transactions?${params.toString()}`);
    },
    [router, searchParams],
  );

  function getRowClass(row: Row<TableRow>): string {
    const category = row.original.item.category_name ?? null;
    const isDeduction = category === "차감";
    const isChildRow = row.depth > 0;

    return cn(
      "border-b hover:bg-muted/30 transition-colors",
      isDeduction && "bg-muted/50 text-muted-foreground",
      isChildRow && "bg-muted/20",
    );
  }

  return (
    <div className="space-y-3">
      <FilterBar
        currentFilters={searchParams}
        onFilterChange={handleFilterChange}
      />

      <div className="text-xs text-muted-foreground">
        총 {total}개 거래
      </div>

      <div className="overflow-x-auto rounded-md border">
        <table className="w-full text-sm">
          <thead>
            {table.getHeaderGroups().map((hg) => (
              <tr key={hg.id} className="bg-muted/50 border-b">
                {hg.headers.map((header) => (
                  <th
                    key={header.id}
                    className="px-3 py-2 text-left text-xs font-medium text-muted-foreground whitespace-nowrap"
                    style={{ width: header.getSize() }}
                  >
                    {header.isPlaceholder
                      ? null
                      : flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.length === 0 ? (
              <tr>
                <td
                  colSpan={columns.length}
                  className="px-3 py-8 text-center text-muted-foreground"
                >
                  거래 내역이 없습니다.
                </td>
              </tr>
            ) : (
              table.getRowModel().rows.map((row) => (
                <tr key={row.id} className={getRowClass(row)}>
                  {row.getVisibleCells().map((cell) => (
                    <td
                      key={cell.id}
                      className="px-3 py-2 whitespace-nowrap"
                      style={{
                        paddingLeft:
                          cell.column.id === "occurred_on" && row.depth > 0
                            ? `${row.depth * 16 + 12}px`
                            : undefined,
                      }}
                    >
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext(),
                      )}
                    </td>
                  ))}
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
