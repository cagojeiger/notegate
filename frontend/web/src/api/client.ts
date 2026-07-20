import { apiErrorFromResponse } from "./errors";
import { downloadBlob, downloadUrl } from "../shared/lib/downloadBlob";

export type ApiClient = {
  get<T>(path: string): Promise<T>;
  post<T>(path: string, body?: unknown): Promise<T>;
  put<T>(path: string, body?: unknown): Promise<T>;
  patch<T>(path: string, body?: unknown): Promise<T>;
  delete<T>(path: string): Promise<T>;
  fetchBlob(path: string): Promise<Blob>;
  download(path: string, filename: string): Promise<void>;
};

type RequestOptions = {
  method: string;
  body?: unknown;
};

export function createApiClient(getApiKey: () => string | null): ApiClient {
  async function request<T>(path: string, options: RequestOptions): Promise<T> {
    const apiKey = getApiKey();
    const headers = new Headers();
    if (apiKey) headers.set("authorization", `Bearer ${apiKey}`);
    if (options.body !== undefined) headers.set("content-type", "application/json");

    const response = await fetch(path, {
      method: options.method,
      headers,
      credentials: "same-origin",
      body: options.body !== undefined ? JSON.stringify(options.body) : undefined
    });

    if (!response.ok) {
      throw await apiErrorFromResponse(response);
    }

    if (response.status === 204) {
      return undefined as T;
    }

    return (await response.json()) as T;
  }

  async function fetchBlob(path: string): Promise<Blob> {
    const apiKey = getApiKey();
    const headers = new Headers();
    if (apiKey) headers.set("authorization", `Bearer ${apiKey}`);
    const response = await fetch(path, { method: "GET", headers, credentials: "same-origin" });
    if (!response.ok) throw await apiErrorFromResponse(response);
    return response.blob();
  }

  return {
    get: <T>(path: string) => request<T>(path, { method: "GET" }),
    post: <T>(path: string, body?: unknown) => request<T>(path, { method: "POST", body }),
    put: <T>(path: string, body?: unknown) => request<T>(path, { method: "PUT", body }),
    patch: <T>(path: string, body?: unknown) => request<T>(path, { method: "PATCH", body }),
    delete: <T>(path: string) => request<T>(path, { method: "DELETE" }),
    fetchBlob,
    async download(path: string, filename: string) {
      if (!getApiKey()) {
        downloadUrl(path, filename);
        return;
      }
      downloadBlob(await fetchBlob(path), filename);
    }
  };
}
