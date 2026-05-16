"use client";

import { useState, useRef, ChangeEvent, DragEvent, useMemo } from "react";
import {
  Upload,
  CheckCircle2,
  AlertTriangle,
  FileSpreadsheet,
  Loader2,
  FolderUp,
  X,
  Clock,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Alert, AlertTitle, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ImportResponse, ImportResponseSchema } from "@/lib/schemas";

const MAX_FILE_SIZE = 20 * 1024 * 1024; // 20 MB

// "2026년 02월.xlsx" 형태만 통과. macOS/한글 IME가 NFD로 자모를 분리해 보내올 수
// 있어 정규식 매칭 전에 NFC 정규화로 합쳐준다.
const FILENAME_RE = /^(\d{4})년\s*(\d{1,2})월\.xlsx$/i;

type FileStatus =
  | { kind: "pending" }
  | { kind: "uploading" }
  | { kind: "success"; result: ImportResponse }
  | { kind: "duplicate" }
  | { kind: "error"; message: string };

type Entry = {
  id: string;
  file: File;
  displayName: string;
  year: number;
  month: number;
  status: FileStatus;
};

type ParsedName = { displayName: string; year: number; month: number };

function parseFilename(name: string): ParsedName | null {
  const normalized = name.normalize("NFC");
  // 경로 포함될 수 있어 basename 추출
  const base = normalized.split("/").pop() ?? normalized;
  const m = FILENAME_RE.exec(base);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  if (!Number.isFinite(year) || month < 1 || month > 12) return null;
  return { displayName: base, year, month };
}

function entryKey(p: ParsedName, size: number) {
  return `${p.year}-${p.month}-${p.displayName}-${size}`;
}

export default function ImportPage() {
  const folderInputRef = useRef<HTMLInputElement>(null);
  const filesInputRef = useRef<HTMLInputElement>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [isUploading, setIsUploading] = useState(false);
  const [isDragOver, setIsDragOver] = useState(false);
  const [skipNotice, setSkipNotice] = useState<string | null>(null);

  function addFiles(incoming: FileList | File[]) {
    const accepted: Entry[] = [];
    let skippedNonMatching = 0;
    let skippedTooBig = 0;

    const seen = new Set(entries.map((e) => entryKey(e, e.file.size)));

    for (const f of Array.from(incoming)) {
      const parsed = parseFilename(f.name);
      if (!parsed) {
        skippedNonMatching += 1;
        continue;
      }
      if (f.size > MAX_FILE_SIZE) {
        skippedTooBig += 1;
        continue;
      }
      const key = entryKey(parsed, f.size);
      if (seen.has(key)) continue;
      seen.add(key);
      accepted.push({
        id: `${key}-${Math.random().toString(36).slice(2, 8)}`,
        file: f,
        displayName: parsed.displayName,
        year: parsed.year,
        month: parsed.month,
        status: { kind: "pending" },
      });
    }

    accepted.sort((a, b) => a.year - b.year || a.month - b.month);
    setEntries((prev) =>
      [...prev, ...accepted].sort((a, b) => a.year - b.year || a.month - b.month),
    );

    const parts: string[] = [];
    if (skippedNonMatching > 0)
      parts.push(`${skippedNonMatching}개는 'YYYY년 MM월.xlsx' 패턴이 아니어서 제외`);
    if (skippedTooBig > 0) parts.push(`${skippedTooBig}개는 20 MB 초과로 제외`);
    setSkipNotice(parts.length ? parts.join(", ") : null);
  }

  function handleFolderChange(e: ChangeEvent<HTMLInputElement>) {
    if (e.target.files) addFiles(e.target.files);
    if (folderInputRef.current) folderInputRef.current.value = "";
  }

  function handleFilesChange(e: ChangeEvent<HTMLInputElement>) {
    if (e.target.files) addFiles(e.target.files);
    if (filesInputRef.current) filesInputRef.current.value = "";
  }

  async function readDataTransfer(dt: DataTransfer): Promise<File[]> {
    const out: File[] = [];
    const items = dt.items;
    // FileSystem API 지원 시 디렉터리를 재귀 탐색해 모든 파일을 펼친다.
    if (
      items &&
      items.length > 0 &&
      typeof items[0].webkitGetAsEntry === "function"
    ) {
      const entries = Array.from(items)
        .map((it) => it.webkitGetAsEntry())
        .filter((e): e is FileSystemEntry => e != null);
      for (const entry of entries) {
        await walkEntry(entry, out);
      }
      return out;
    }
    // fallback: 폴더 드롭 미지원 환경은 dataTransfer.files만 사용
    return Array.from(dt.files);
  }

  async function walkEntry(entry: FileSystemEntry, out: File[]) {
    if (entry.isFile) {
      const fileEntry = entry as FileSystemFileEntry;
      await new Promise<void>((resolve) => {
        fileEntry.file(
          (f) => {
            out.push(f);
            resolve();
          },
          () => resolve(),
        );
      });
    } else if (entry.isDirectory) {
      const dirEntry = entry as FileSystemDirectoryEntry;
      const reader = dirEntry.createReader();
      // readEntries는 한 번에 최대 100개만 반환할 수 있어 빌 때까지 반복.
      // (실무에서 거의 발생하지 않지만 폴더 깊이/파일 수가 많을 때 누락 방지)
      while (true) {
        const batch: FileSystemEntry[] = await new Promise((resolve) => {
          reader.readEntries(
            (es) => resolve(es),
            () => resolve([]),
          );
        });
        if (batch.length === 0) break;
        for (const child of batch) await walkEntry(child, out);
      }
    }
  }

  async function handleDrop(e: DragEvent<HTMLDivElement>) {
    e.preventDefault();
    setIsDragOver(false);
    if (isUploading) return;
    const files = await readDataTransfer(e.dataTransfer);
    addFiles(files);
  }

  function removeEntry(id: string) {
    setEntries((prev) => prev.filter((e) => e.id !== id));
  }

  function updateEntry(id: string, status: FileStatus) {
    setEntries((prev) => prev.map((e) => (e.id === id ? { ...e, status } : e)));
  }

  async function uploadOne(entry: Entry): Promise<FileStatus> {
    const form = new FormData();
    form.append("file", entry.file, entry.displayName);

    try {
      const res = await fetch("/api/import", { method: "POST", body: form });
      if (res.status === 409) return { kind: "duplicate" };
      if (!res.ok) {
        const text = await res.text().catch(() => "");
        let detail = text || `HTTP ${res.status}`;
        try {
          const j = JSON.parse(text) as { detail?: string };
          if (j.detail) detail = j.detail;
        } catch {
          /* keep raw text */
        }
        return { kind: "error", message: detail };
      }
      const raw: unknown = await res.json();
      const result = ImportResponseSchema.parse(raw);
      return { kind: "success", result };
    } catch (err) {
      return {
        kind: "error",
        message: err instanceof Error ? err.message : "네트워크 오류",
      };
    }
  }

  async function handleUploadAll() {
    if (isUploading) return;
    const targets = entries.filter(
      (e) => e.status.kind === "pending" || e.status.kind === "error",
    );
    if (targets.length === 0) return;

    setIsUploading(true);
    for (const t of targets) {
      updateEntry(t.id, { kind: "uploading" });
      const next = await uploadOne(t);
      updateEntry(t.id, next);
    }
    setIsUploading(false);
  }

  function handleReset() {
    if (isUploading) return;
    setEntries([]);
    setSkipNotice(null);
  }

  const summary = useMemo(() => {
    const total = entries.length;
    const success = entries.filter((e) => e.status.kind === "success").length;
    const duplicate = entries.filter((e) => e.status.kind === "duplicate").length;
    const errored = entries.filter((e) => e.status.kind === "error").length;
    const pending = entries.filter(
      (e) => e.status.kind === "pending" || e.status.kind === "uploading",
    ).length;
    return { total, success, duplicate, errored, pending };
  }, [entries]);

  const hasUploadable = entries.some(
    (e) => e.status.kind === "pending" || e.status.kind === "error",
  );

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <Upload className="h-6 w-6" />
        <h1 className="text-2xl font-bold">엑셀 임포트</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">가계부 파일 일괄 업로드</CardTitle>
          <CardDescription>
            폴더를 통째로 선택하면 하위의 <code className="font-mono">YYYY년 MM월.xlsx</code> 파일만 자동 추려서 순차 업로드합니다. 개별 파일도 여러 개 선택할 수 있고, 폴더/파일을 끌어놓아도 됩니다. (파일당 최대 20 MB)
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div
            className={`border-2 border-dashed rounded-lg p-8 text-center transition-colors ${
              isDragOver ? "border-primary bg-primary/5" : "hover:border-primary/50"
            } ${isUploading ? "opacity-60 pointer-events-none" : ""}`}
            onDragOver={(e) => {
              e.preventDefault();
              setIsDragOver(true);
            }}
            onDragLeave={() => setIsDragOver(false)}
            onDrop={handleDrop}
            aria-label="드롭 영역"
          >
            <FileSpreadsheet className="h-10 w-10 mx-auto text-muted-foreground mb-3" />
            <p className="text-sm font-medium mb-1">폴더나 파일을 끌어놓거나 아래에서 선택하세요</p>
            <p className="text-xs text-muted-foreground mb-4">
              패턴: <span className="font-mono">YYYY년 MM월.xlsx</span> (예: 2026년 02월.xlsx)
            </p>
            <div className="flex flex-wrap items-center justify-center gap-2">
              <Button
                type="button"
                variant="secondary"
                onClick={() => folderInputRef.current?.click()}
                disabled={isUploading}
              >
                <FolderUp className="h-4 w-4" />
                <span>폴더 선택</span>
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => filesInputRef.current?.click()}
                disabled={isUploading}
              >
                <FileSpreadsheet className="h-4 w-4" />
                <span>파일 선택</span>
              </Button>
            </div>

            {/* 폴더 선택: webkitdirectory — Chrome/Edge/Safari 지원 */}
            <input
              ref={folderInputRef}
              type="file"
              multiple
              // @ts-expect-error: non-standard but widely supported folder picker attrs
              webkitdirectory=""
              directory=""
              onChange={handleFolderChange}
              className="hidden"
              aria-hidden="true"
            />
            <input
              ref={filesInputRef}
              type="file"
              multiple
              accept=".xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
              onChange={handleFilesChange}
              className="hidden"
              aria-hidden="true"
            />
          </div>

          {skipNotice && (
            <Alert variant="warning">
              <AlertTriangle className="h-4 w-4" />
              <AlertTitle>일부 파일은 제외되었습니다</AlertTitle>
              <AlertDescription>{skipNotice}</AlertDescription>
            </Alert>
          )}

          {entries.length > 0 && (
            <>
              <div className="flex items-center justify-between gap-2">
                <div className="text-sm text-muted-foreground">
                  대상 <span className="font-semibold text-foreground">{summary.total}</span>개
                  · 성공 <span className="font-semibold text-green-700">{summary.success}</span>
                  · 중복 <span className="font-semibold text-yellow-700">{summary.duplicate}</span>
                  · 실패 <span className="font-semibold text-destructive">{summary.errored}</span>
                  · 대기 <span className="font-semibold">{summary.pending}</span>
                </div>
                <div className="flex gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={handleReset}
                    disabled={isUploading}
                  >
                    초기화
                  </Button>
                  <Button
                    type="button"
                    onClick={handleUploadAll}
                    disabled={!hasUploadable || isUploading}
                  >
                    {isUploading ? (
                      <>
                        <Loader2 className="h-4 w-4 animate-spin" />
                        <span>업로드 중...</span>
                      </>
                    ) : (
                      <>
                        <Upload className="h-4 w-4" />
                        <span>전체 업로드 시작</span>
                      </>
                    )}
                  </Button>
                </div>
              </div>

              <div className="border rounded-md divide-y">
                {entries.map((e) => (
                  <EntryRow
                    key={e.id}
                    entry={e}
                    onRemove={() => removeEntry(e.id)}
                    disabled={isUploading}
                  />
                ))}
              </div>
            </>
          )}
        </CardContent>
      </Card>

      {summary.success > 0 && !isUploading && (
        <div className="flex gap-2">
          <Button asChild>
            <a href="/transactions">거래 내역 보기</a>
          </Button>
        </div>
      )}
    </div>
  );
}

function EntryRow({
  entry,
  onRemove,
  disabled,
}: {
  entry: Entry;
  onRemove: () => void;
  disabled: boolean;
}) {
  const { status } = entry;

  let icon = <Clock className="h-4 w-4 text-muted-foreground" />;
  let statusBadge: React.ReactNode = <Badge variant="outline">대기</Badge>;

  if (status.kind === "uploading") {
    icon = <Loader2 className="h-4 w-4 animate-spin text-primary" />;
    statusBadge = <Badge variant="secondary">업로드 중</Badge>;
  } else if (status.kind === "success") {
    icon = <CheckCircle2 className="h-4 w-4 text-green-600" />;
    statusBadge = (
      <Badge
        variant="outline"
        className="border-green-200 bg-green-50 text-green-700"
      >
        완료
      </Badge>
    );
  } else if (status.kind === "duplicate") {
    icon = <AlertTriangle className="h-4 w-4 text-yellow-600" />;
    statusBadge = (
      <Badge
        variant="outline"
        className="border-yellow-200 bg-yellow-50 text-yellow-700"
      >
        중복
      </Badge>
    );
  } else if (status.kind === "error") {
    icon = <AlertTriangle className="h-4 w-4 text-destructive" />;
    statusBadge = <Badge variant="destructive">실패</Badge>;
  }

  return (
    <div className="flex items-start gap-3 p-3 text-sm">
      <div className="pt-0.5">{icon}</div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-medium truncate">{entry.displayName}</span>
          {statusBadge}
          <span className="text-xs text-muted-foreground">
            {(entry.file.size / 1024).toFixed(1)} KB
          </span>
        </div>
        {status.kind === "success" && (
          <div className="mt-1 text-xs text-muted-foreground">
            거래 {status.result.transactions_inserted}건 삽입 · 엑셀 {status.result.row_count}행
            {status.result.integrity_warnings.length > 0 && (
              <span className="text-yellow-700">
                {" "}
                · 합계 불일치 {status.result.integrity_warnings.length}건
              </span>
            )}
            {status.result.unresolved_aliases.length > 0 && (
              <span className="text-yellow-700">
                {" "}
                · 미해결 별칭 {status.result.unresolved_aliases.length}건
              </span>
            )}
          </div>
        )}
        {status.kind === "duplicate" && (
          <div className="mt-1 text-xs text-muted-foreground">
            동일한 파일이 이미 임포트되어 있습니다 (SHA-256 해시 중복).
          </div>
        )}
        {status.kind === "error" && (
          <div className="mt-1 text-xs text-destructive break-words">{status.message}</div>
        )}
      </div>
      {(status.kind === "pending" ||
        status.kind === "error" ||
        status.kind === "duplicate") && (
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          className="text-muted-foreground hover:text-foreground disabled:opacity-50"
          aria-label={`${entry.displayName} 제거`}
        >
          <X className="h-4 w-4" />
        </button>
      )}
    </div>
  );
}
