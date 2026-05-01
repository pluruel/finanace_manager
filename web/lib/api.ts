import "server-only";
import { cookies } from "next/headers";
import { z } from "zod";

/**
 * 서버 컴포넌트 / Route Handler 전용 fetch wrapper.
 * "server-only" import로 클라이언트 번들에 포함 시 빌드 에러가 발생한다.
 *
 * access 토큰을 쿠키에서 꺼내 `Cookie: Authorization=Bearer <token>` 헤더로 전달한다.
 * 클라이언트에 access 토큰을 노출하지 않는다.
 */

function getApiBase(): string {
  // 서버 측 내부 URL 우선, 없으면 공개 URL 사용
  return (
    process.env.API_BASE_URL_INTERNAL ??
    process.env.NEXT_PUBLIC_API_BASE_URL ??
    "http://localhost:8000"
  );
}

type ApiFetchOptions<T> = RequestInit & {
  // input 타입을 unknown으로 열어둔다 (z.lazy + transform 조합에서 input 추론이 꼬임)
  schema?: z.ZodType<T, z.ZodTypeDef, unknown>;
};

/**
 * 단일 fetch 실행 (토큰 포함). 내부 헬퍼.
 */
async function doFetch(url: string, init: RequestInit, accessToken: string | undefined): Promise<Response> {
  const extraHeaders: Record<string, string> =
    init.headers && !Array.isArray(init.headers) && !(init.headers instanceof Headers)
      ? (init.headers as Record<string, string>)
      : {};

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...extraHeaders,
  };

  // access 토큰이 있으면 Cookie 헤더에 Bearer 스킴과 함께 전달
  // MSA_INTEGRATION.md: Cookie: Authorization=Bearer <token> 형식 필수
  if (accessToken) {
    headers["Cookie"] = `Authorization=Bearer ${accessToken}`;
  }

  return fetch(url, {
    ...init,
    headers,
    cache: "no-store",
  });
}

export async function apiFetch<T = unknown>(
  path: string,
  init?: ApiFetchOptions<T>,
): Promise<T> {
  const base = getApiBase();
  const url = `${base}${path}`;

  // access 쿠키에서 토큰 읽기 (서버 컴포넌트 전용)
  const cookieStore = await cookies();
  const accessToken = cookieStore.get("access")?.value;

  const { schema, ...fetchInit } = init ?? {};

  const response = await doFetch(url, fetchInit, accessToken);

  // 401은 middleware가 사전 refresh를 담당한다.
  // RSC 레이어에서 401이 도달했다면 refresh도 실패한 상황이므로 그대로 throw한다.
  // 호출자(page.tsx)가 리다이렉트 여부를 결정한다.
  if (!response.ok) {
    const errorText = await response.text().catch(() => "unknown error");
    throw new ApiError(response.status, errorText);
  }

  const data = (await response.json()) as unknown;

  if (schema) {
    return schema.parse(data);
  }

  return data as T;
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/**
 * multipart/form-data 전송 (임포트용).
 * Content-Type은 fetch가 자동으로 boundary와 함께 설정하도록 헤더에서 제거.
 */
export async function apiFetchFormData<T = unknown>(
  path: string,
  formData: FormData,
  schema?: z.ZodType<T>,
): Promise<T> {
  const base = getApiBase();
  const url = `${base}${path}`;

  const cookieStore = await cookies();
  const accessToken = cookieStore.get("access")?.value;

  const headers: Record<string, string> = {};

  if (accessToken) {
    headers["Cookie"] = `Authorization=Bearer ${accessToken}`;
  }

  const response = await fetch(url, {
    method: "POST",
    headers,
    body: formData,
    cache: "no-store",
  });

  if (!response.ok) {
    const errorText = await response.text().catch(() => "unknown error");
    throw new ApiError(response.status, errorText);
  }

  const data = (await response.json()) as unknown;

  if (schema) {
    return schema.parse(data);
  }

  return data as T;
}
