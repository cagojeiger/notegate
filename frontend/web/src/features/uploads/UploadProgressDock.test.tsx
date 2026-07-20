import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { UploadTask } from "./UploadProvider";
import { UploadProgressDock } from "./UploadProgressDock";

const mocks = vi.hoisted(() => ({ useUploadManager: vi.fn() }));

vi.mock("./UploadProvider", () => ({ useUploadManager: mocks.useUploadManager }));

describe("UploadProgressDock", () => {
  beforeEach(() => mocks.useUploadManager.mockReset());

  it("stays hidden when there are no transfers", () => {
    mocks.useUploadManager.mockReturnValue(manager());
    const { container } = render(<UploadProgressDock />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows each transfer with its captured destination and progress", async () => {
    const cancelUpload = vi.fn();
    mocks.useUploadManager.mockReturnValue(manager({
      tasks: [
        task({ uploadedBytes: 4 }),
        task({ id: "upload-2", name: "assets.tar", destinationPath: "/Assets", file: new File(["0123456789"], "assets.tar") })
      ],
      activeCount: 2,
      cancelUpload
    }));

    render(<UploadProgressDock />);

    expect(screen.getByText("2 active")).toBeInTheDocument();
    expect(screen.getByText("Daily/Reports")).toBeInTheDocument();
    expect(screen.getByText("Daily/Assets")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "archive.zip upload progress" })).toHaveAttribute("aria-valuenow", "40");

    await userEvent.click(screen.getByRole("button", { name: "Cancel upload archive.zip" }));
    expect(cancelUpload).toHaveBeenCalledWith("upload-1");
  });

  it("keeps failed transfers actionable without a misleading progress bar", async () => {
    const retryUpload = vi.fn();
    const dismissUpload = vi.fn();
    mocks.useUploadManager.mockReturnValue(manager({
      tasks: [task({ status: "failed", error: "network unavailable" })],
      failedCount: 1,
      retryUpload,
      dismissUpload
    }));

    render(<UploadProgressDock />);

    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByText("network unavailable")).toBeVisible();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel upload archive.zip" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Retry upload archive.zip" }));
    await userEvent.click(screen.getByRole("button", { name: "Dismiss upload archive.zip" }));
    expect(retryUpload).toHaveBeenCalledWith("upload-1");
    expect(dismissUpload).toHaveBeenCalledWith("upload-1");
  });

  it("collapses the transfer list without leaving the current screen", async () => {
    mocks.useUploadManager.mockReturnValue(manager({ tasks: [task()], activeCount: 1 }));
    render(<UploadProgressDock />);

    const toggle = screen.getByRole("button", { name: "Collapse uploads" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");

    await userEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAccessibleName("Expand uploads");
    expect(screen.queryByText("archive.zip")).not.toBeInTheDocument();
  });
});

function manager(overrides: Record<string, unknown> = {}) {
  return {
    tasks: [],
    activeCount: 0,
    failedCount: 0,
    startUpload: vi.fn(),
    cancelUpload: vi.fn(),
    retryUpload: vi.fn(),
    dismissUpload: vi.fn(),
    ...overrides
  };
}

function task(overrides: Partial<UploadTask> = {}): UploadTask {
  return {
    id: "upload-1",
    spaceId: "space-1",
    spaceName: "Daily",
    destinationPath: "/Reports",
    parentNodeId: "parent-1",
    name: "archive.zip",
    file: new File(["0123456789"], "archive.zip"),
    status: "uploading",
    uploadedBytes: 0,
    error: null,
    ...overrides
  };
}
