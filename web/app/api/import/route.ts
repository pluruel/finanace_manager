import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const API_BASE =
  process.env.API_BASE_URL_INTERNAL ??
  process.env.NEXT_PUBLIC_API_BASE_URL ??
  "http://localhost:8000";

/**
 * POST /api/import
 *
 * 클라이언트에서 multipart/form-data로 xlsx 파일을 받아
 * 백엔드 POST /api/import에 access 쿠키를 첨부하여 forward한다.
 *
 * request.body (ReadableStream)를 직접 forward해 메모리 버퍼링을 방지한다.
 * Content-Type 헤더(boundary 포함)를 원본 그대로 전달해 multipart 파싱이 깨지지 않게 한다.
 * 클라이언트가 백엔드에 직접 접근하지 않으므로 access 토큰이 노출되지 않는다.
 */
export async function POST(request: NextRequest) {
  const accessToken = request.cookies.get("access")?.value;

  if (!accessToken) {
    return NextResponse.json({ detail: "Not authenticated" }, { status: 401 });
  }

  const contentType = request.headers.get("content-type");
  if (!contentType || !contentType.includes("multipart/form-data")) {
    return NextResponse.json({ detail: "multipart/form-data required" }, { status: 400 });
  }

  // 백엔드에 access 쿠키를 Cookie 헤더로 전달
  // MSA_INTEGRATION.md: Cookie: Authorization=Bearer <token>
  let backendRes: Response;
  try {
    backendRes = await fetch(`${API_BASE}/api/import`, {
      method: "POST",
      headers: {
        Cookie: `Authorization=Bearer ${accessToken}`,
        // Content-Type을 원본 그대로 전달 (boundary 보존 필수)
        "Content-Type": contentType,
      },
      // request.body를 ReadableStream으로 직접 forward — 메모리 버퍼링 없음
      // duplex: "half"는 Node.js fetch에서 스트리밍 body 전송에 필요하지만 TS 타입 정의에 없음
      body: request.body,
      duplex: "half",
    } as RequestInit);
  } catch (err) {
    console.error("[api/import] fetch error:", err);
    return NextResponse.json(
      { detail: "Backend service unavailable" },
      { status: 502 },
    );
  }

  const responseText = await backendRes.text();

  return new NextResponse(responseText, {
    status: backendRes.status,
    headers: {
      "Content-Type": "application/json",
    },
  });
}
