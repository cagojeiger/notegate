import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getAudioPreviewUrl } from "../../api/files";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { canPreviewAudio, useAudioPreviewUrl } from "./useAudioPreviewQuery";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/files", () => ({
  filePreviewStaleTime: vi.fn(() => 60_000),
  getAudioPreviewUrl: vi.fn()
}));

describe("useAudioPreviewUrl", () => {
  beforeEach(() => {
    vi.mocked(getAudioPreviewUrl).mockReset();
  });

  it("loads a dedicated URL only for server-verified audio", async () => {
    const queryClient = createTestQueryClient();
    const audioNode = fileNode({ file_media_kind: "audio" });
    vi.mocked(getAudioPreviewUrl).mockResolvedValue({
      url: "https://storage.example/meeting.webm",
      media_type: "audio/webm",
      expires_at: "2026-06-13T00:15:00Z"
    });

    const { result } = renderHook(() => useAudioPreviewUrl(audioNode), {
      wrapper: createQueryWrapper(queryClient)
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(getAudioPreviewUrl).toHaveBeenCalledWith(
      mockClient,
      "space-1",
      "file-1"
    );
    expect(queryClient.getQueryData(
      queryKeys.audioPreviewUrl("space-1", "file-1")
    )).toMatchObject({ media_type: "audio/webm" });
    expect(queryClient.getQueryCache().find({
      queryKey: queryKeys.audioPreviewUrl("space-1", "file-1"),
      exact: true
    })?.options.gcTime).toBe(15 * 60 * 1_000);
  });

  it("does not issue inline URLs for unverified or client-encrypted files", () => {
    const declaredAudio = fileNode({
      media_type: "audio/webm",
      file_media_kind: undefined
    });
    const encryptedAudio = fileNode({
      file_media_kind: "audio",
      encryption_mode: "client"
    });

    renderHook(() => useAudioPreviewUrl(declaredAudio), { wrapper: createQueryWrapper() });
    renderHook(() => useAudioPreviewUrl(encryptedAudio), { wrapper: createQueryWrapper() });

    expect(canPreviewAudio(declaredAudio)).toBe(false);
    expect(canPreviewAudio(encryptedAudio)).toBe(false);
    expect(getAudioPreviewUrl).not.toHaveBeenCalled();
  });
});

function createQueryWrapper(queryClient = createTestQueryClient()) {
  return function QueryWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function fileNode(overrides: Partial<RestNode>): RestNode {
  return makeRestNode({
    id: "file-1",
    space_id: "space-1",
    kind: "file",
    name: "meeting.webm",
    path: "/meeting.webm",
    media_type: "audio/webm",
    encryption_mode: "none",
    ...overrides
  });
}
