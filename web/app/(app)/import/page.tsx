"use client";

import { useState, useRef, ChangeEvent, FormEvent } from "react";
import { Upload, CheckCircle2, AlertTriangle, FileSpreadsheet, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Alert, AlertTitle, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ImportResponse, ImportResponseSchema } from "@/lib/schemas";

const MAX_FILE_SIZE = 20 * 1024 * 1024; // 20 MB
const ALLOWED_MIME = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

type ImportState =
  | { status: "idle" }
  | { status: "uploading" }
  | { status: "success"; result: ImportResponse }
  | { status: "duplicate" }
  | { status: "error"; message: string };

export default function ImportPage() {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [state, setState] = useState<ImportState>({ status: "idle" });

  function handleFileChange(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0] ?? null;
    if (!file) {
      setSelectedFile(null);
      setState({ status: "idle" });
      return;
    }

    // 사전 검증: 확장자 및 MIME 타입
    const ext = file.name.split(".").pop()?.toLowerCase();
    if (ext !== "xlsx" && file.type !== ALLOWED_MIME) {
      setState({ status: "error", message: ".xlsx 파일만 업로드할 수 있습니다." });
      setSelectedFile(null);
      if (fileInputRef.current) fileInputRef.current.value = "";
      return;
    }

    // 사전 검증: 파일 크기
    if (file.size > MAX_FILE_SIZE) {
      setState({
        status: "error",
        message: `파일 크기가 너무 큽니다. 최대 20 MB까지 허용됩니다. (현재: ${(file.size / 1024 / 1024).toFixed(1)} MB)`,
      });
      setSelectedFile(null);
      if (fileInputRef.current) fileInputRef.current.value = "";
      return;
    }

    setSelectedFile(file);
    setState({ status: "idle" });
  }

  async function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (!selectedFile) return;

    setState({ status: "uploading" });

    const formData = new FormData();
    formData.append("file", selectedFile);

    try {
      const res = await fetch("/api/import", {
        method: "POST",
        body: formData,
      });

      if (res.status === 409) {
        setState({ status: "duplicate" });
        return;
      }

      if (!res.ok) {
        const text = await res.text().catch(() => "알 수 없는 오류");
        let detail = text;
        try {
          const json = JSON.parse(text) as { detail?: string };
          detail = json.detail ?? text;
        } catch {
          // 파싱 실패 시 원본 텍스트 사용
        }
        setState({ status: "error", message: detail });
        return;
      }

      // zod로 응답 검증 (unsafe 캐스팅 제거)
      const raw: unknown = await res.json();
      const result = ImportResponseSchema.parse(raw);
      setState({ status: "success", result });
    } catch (err) {
      setState({
        status: "error",
        message: err instanceof Error ? err.message : "네트워크 오류가 발생했습니다.",
      });
    }
  }

  function handleReset() {
    setSelectedFile(null);
    setState({ status: "idle" });
    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  }

  const isUploading = state.status === "uploading";

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <Upload className="h-6 w-6" />
        <h1 className="text-2xl font-bold">엑셀 임포트</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">가계부 파일 업로드</CardTitle>
          <CardDescription>
            월별 가계부 엑셀 파일(.xlsx)을 업로드하면 자동으로 파싱·정규화합니다.
            최대 20 MB까지 허용됩니다.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div
              className="border-2 border-dashed rounded-lg p-8 text-center cursor-pointer hover:border-primary/50 transition-colors"
              onClick={() => fileInputRef.current?.click()}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  fileInputRef.current?.click();
                }
              }}
              role="button"
              tabIndex={0}
              aria-label="파일 선택 영역"
            >
              <FileSpreadsheet className="h-10 w-10 mx-auto text-muted-foreground mb-3" />
              {selectedFile ? (
                <div className="space-y-1">
                  <p className="text-sm font-medium">{selectedFile.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {(selectedFile.size / 1024).toFixed(1)} KB
                  </p>
                </div>
              ) : (
                <div className="space-y-1">
                  <p className="text-sm font-medium">클릭하여 파일 선택</p>
                  <p className="text-xs text-muted-foreground">
                    .xlsx 파일만 지원합니다 (예: 2026년 02월.xlsx)
                  </p>
                </div>
              )}
              <input
                ref={fileInputRef}
                type="file"
                accept=".xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                onChange={handleFileChange}
                className="hidden"
                aria-hidden="true"
              />
            </div>

            <div className="flex gap-2">
              <Button
                type="submit"
                disabled={!selectedFile || isUploading}
                className="flex-1"
              >
                {isUploading ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    <span>업로드 중...</span>
                  </>
                ) : (
                  <>
                    <Upload className="h-4 w-4" />
                    <span>임포트 시작</span>
                  </>
                )}
              </Button>
              {selectedFile && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleReset}
                  disabled={isUploading}
                >
                  초기화
                </Button>
              )}
            </div>
          </form>
        </CardContent>
      </Card>

      {/* 결과 카드 */}
      {state.status === "duplicate" && (
        <Alert variant="warning">
          <AlertTriangle className="h-4 w-4" />
          <AlertTitle>이미 임포트된 파일</AlertTitle>
          <AlertDescription>
            동일한 파일이 이미 임포트되어 있습니다 (SHA-256 해시 중복).
            같은 파일을 다시 업로드할 수 없습니다.
          </AlertDescription>
        </Alert>
      )}

      {state.status === "error" && (
        <Alert variant="destructive">
          <AlertTriangle className="h-4 w-4" />
          <AlertTitle>임포트 실패</AlertTitle>
          <AlertDescription>{state.message}</AlertDescription>
        </Alert>
      )}

      {state.status === "success" && (
        <div className="space-y-4">
          <Alert variant="success">
            <CheckCircle2 className="h-4 w-4" />
            <AlertTitle>임포트 완료</AlertTitle>
            <AlertDescription>
              {state.result.year}년 {state.result.month}월 데이터를 성공적으로 임포트했습니다.
            </AlertDescription>
          </Alert>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">임포트 결과</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                <ResultStat
                  label="배치 ID"
                  value={state.result.batch_id.slice(0, 8) + "..."}
                />
                <ResultStat
                  label="대상 기간"
                  value={`${state.result.year}년 ${state.result.month}월`}
                />
                <ResultStat
                  label="엑셀 행수"
                  value={String(state.result.row_count)}
                />
                <ResultStat
                  label="삽입된 거래"
                  value={String(state.result.transactions_inserted)}
                />
              </div>

              {/* 미해결 별칭 목록 */}
              {state.result.unresolved_aliases.length > 0 && (
                <div className="space-y-2">
                  <Alert variant="warning">
                    <AlertTriangle className="h-4 w-4" />
                    <AlertTitle>미해결 별칭</AlertTitle>
                    <AlertDescription>
                      {state.result.unresolved_aliases.length}개의 별칭이 자동 매핑되지 않았습니다.
                      정규화 페이지에서 수동으로 검토해주세요.
                    </AlertDescription>
                  </Alert>
                  <div className="overflow-x-auto">
                    <table className="w-full text-xs border-collapse">
                      <thead>
                        <tr className="bg-muted">
                          <th className="border px-2 py-1 text-left">범위</th>
                          <th className="border px-2 py-1 text-left">원문</th>
                          <th className="border px-2 py-1 text-left">정규화 키</th>
                        </tr>
                      </thead>
                      <tbody>
                        {state.result.unresolved_aliases.map((a, i) => (
                          <tr key={i} className="hover:bg-muted/50">
                            <td className="border px-2 py-1">
                              <Badge variant="outline" className="text-xs">{a.scope}</Badge>
                            </td>
                            <td className="border px-2 py-1 font-mono">{a.raw_text}</td>
                            <td className="border px-2 py-1 font-mono text-muted-foreground">{a.norm_key}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}

              {/* 합계 무결성 */}
              {state.result.integrity_warnings.length === 0 ? (
                <div className="flex items-center gap-2 p-3 bg-green-50 rounded-md border border-green-200">
                  <CheckCircle2 className="h-4 w-4 text-green-600 shrink-0" />
                  <span className="text-sm text-green-700 font-medium">
                    합계 무결성 통과 — 모든 그룹의 합계가 일치합니다.
                  </span>
                </div>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <AlertTriangle className="h-4 w-4 text-yellow-600" />
                    <span className="text-sm font-medium text-yellow-700">
                      합계 불일치 그룹 {state.result.integrity_warnings.length}건
                    </span>
                  </div>
                  <div className="overflow-x-auto">
                    <table className="w-full text-xs border-collapse">
                      <thead>
                        <tr className="bg-muted">
                          <th className="border px-2 py-1 text-left">그룹 ID</th>
                          <th className="border px-2 py-1 text-right">헤더 합계</th>
                          <th className="border px-2 py-1 text-right">라인 합계</th>
                        </tr>
                      </thead>
                      <tbody>
                        {state.result.integrity_warnings.map((w) => (
                          <tr key={w.group_id} className="hover:bg-muted/50">
                            <td className="border px-2 py-1 font-mono">
                              {w.group_id.slice(0, 8)}...
                            </td>
                            <td className="border px-2 py-1 text-right">
                              {w.header_total}
                            </td>
                            <td className="border px-2 py-1 text-right text-destructive">
                              {w.lines_sum}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>

          <div className="flex gap-2">
            <Button variant="outline" onClick={handleReset}>
              새 파일 임포트
            </Button>
            <Button asChild>
              <a href="/transactions">거래 내역 보기</a>
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function ResultStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="p-3 rounded-lg bg-muted/50 space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="text-sm font-semibold">{value}</p>
    </div>
  );
}
