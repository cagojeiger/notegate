import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UploadProvider, useUploadManager } from "./UploadProvider";

const mocks = vi.hoisted(() => ({
  abortFileUpload: vi.fn(),
  beginFileUpload: vi.fn(),
  completeFileUpload: vi.fn(),
  transferFile: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/files", () => ({
  abortFileUpload: mocks.abortFileUpload,
  beginFileUpload: mocks.beginFileUpload,
  completeFileUpload: mocks.completeFileUpload,
  transferFile: mocks.transferFile
}));

const uploadResponse = {
  upload_id: "server-upload-1",
  transfer: { mode: "single", url: "https://objects.test/upload", headers: {} }
};

describe("UploadProvider", () => {
  beforeEach(() => {
    window.localStorage.clear();
    mocks.beginFileUpload.mockReset().mockResolvedValue(uploadResponse);
    mocks.abortFileUpload.mockReset().mockResolvedValue(undefined);
    mocks.completeFileUpload.mockReset().mockResolvedValue({ node: { id: "node-1" } });
    mocks.transferFile.mockReset();
  });

  it("tracks transfer progress and completes without changing workbench state", async () => {
    const transfer = deferred<void>();
    let reportProgress: ((uploadedBytes: number, totalBytes: number) => void) | undefined;
    mocks.transferFile.mockImplementation((_client, _spaceId, _upload, _file, options) => {
      reportProgress = options.onProgress;
      return transfer.promise;
    });
    const { result, queryClient } = renderUploadManager();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    act(() => { result.current.startUpload(input()); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("uploading"));

    act(() => { reportProgress?.(5, 10); });
    expect(result.current.tasks[0]?.uploadedBytes).toBe(5);
    act(() => { reportProgress?.(12, 10); });
    expect(result.current.tasks[0]?.uploadedBytes).toBe(12);

    await act(async () => { transfer.resolve(); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("completed"));

    expect(mocks.completeFileUpload).toHaveBeenCalledWith(expect.anything(), "space-1", "server-upload-1", undefined);
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["spaces"], exact: true });
    expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ["spaces", "space-1"] });
  });

  it("removes a transfer when the user cancels it", async () => {
    mocks.transferFile.mockImplementation((_client, _spaceId, _upload, _file, options) => new Promise<void>((_resolve, reject) => {
      options.signal.addEventListener("abort", () => reject(new DOMException("canceled", "AbortError")), { once: true });
    }));
    const { result } = renderUploadManager();

    let taskId = "";
    act(() => { taskId = result.current.startUpload(input()); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("uploading"));

    act(() => { result.current.cancelUpload(taskId); });

    await waitFor(() => expect(result.current.tasks).toHaveLength(0));
    expect(mocks.abortFileUpload).toHaveBeenCalledWith(expect.anything(), "space-1", "server-upload-1");
    expect(mocks.completeFileUpload).not.toHaveBeenCalled();
  });

  it("restarts a failed transfer from the begin step only once", async () => {
    mocks.transferFile
      .mockRejectedValueOnce(new Error("network unavailable"))
      .mockResolvedValueOnce(undefined);
    const { result } = renderUploadManager();

    let taskId = "";
    act(() => { taskId = result.current.startUpload(input()); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("failed"));
    expect(mocks.abortFileUpload).toHaveBeenCalledWith(expect.anything(), "space-1", "server-upload-1");

    act(() => {
      result.current.retryUpload(taskId);
      result.current.retryUpload(taskId);
    });

    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("completed"));
    expect(mocks.beginFileUpload).toHaveBeenCalledTimes(2);
    expect(mocks.transferFile).toHaveBeenCalledTimes(2);
  });

  it("does not dismiss a failed task after its retry has started", async () => {
    const retry = deferred<void>();
    mocks.transferFile
      .mockRejectedValueOnce(new Error("network unavailable"))
      .mockReturnValueOnce(retry.promise);
    const { result } = renderUploadManager();

    let taskId = "";
    act(() => { taskId = result.current.startUpload(input()); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("failed"));

    act(() => {
      result.current.retryUpload(taskId);
      result.current.dismissUpload(taskId);
    });

    expect(result.current.tasks).toHaveLength(1);
    await act(async () => { retry.resolve(); });
    await waitFor(() => expect(result.current.tasks[0]?.status).toBe("completed"));
  });
});

function renderUploadManager() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <UploadProvider>{children}</UploadProvider>
    </QueryClientProvider>
  );
  return { ...renderHook(() => useUploadManager(), { wrapper }), queryClient };
}

function input() {
  return {
    spaceId: "space-1",
    spaceName: "Daily",
    destinationPath: "/Reports",
    parentNodeId: "parent-1",
    name: "archive.zip",
    file: new File(["0123456789"], "archive.zip", { type: "application/zip" })
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
