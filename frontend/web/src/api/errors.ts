export class ApiError extends Error {
  readonly status: number;
  readonly kind: string | null;

  constructor(message: string, status: number, kind: string | null = null) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.kind = kind;
  }
}

type ErrorLikeBody = {
  error?: {
    message?: string;
    kind?: string;
  };
  message?: string;
  kind?: string;
};

export async function apiErrorFromResponse(response: Response): Promise<ApiError> {
  let body: ErrorLikeBody | null = null;
  try {
    body = (await response.json()) as ErrorLikeBody;
  } catch {
    body = null;
  }

  const message = body?.error?.message ?? body?.message ?? response.statusText ?? "Request failed";
  const kind = body?.error?.kind ?? body?.kind ?? null;
  return new ApiError(message, response.status, kind);
}
