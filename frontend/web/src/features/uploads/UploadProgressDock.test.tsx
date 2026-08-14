import { render, screen, waitFor } from "@testing-library/react";
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
    render(<UploadProgressDock />);
    expect(screen.queryByRole("region", { name: "File uploads" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
  });

  it("announces status transitions without repeating progress updates", async () => {
    let current = manager();
    mocks.useUploadManager.mockImplementation(() => current);
    const { rerender } = render(<UploadProgressDock />);
    const liveRegion = screen.getByRole("status");

    current = manager({ tasks: [task({ status: "queued" })], queuedCount: 1 });
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toHaveTextContent("archive.zip: Queued"));

    current = manager({ tasks: [task({ status: "uploading", uploadedBytes: 2 })], activeCount: 1 });
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toHaveTextContent("archive.zip: Uploading"));

    current = manager({ tasks: [task({ status: "uploading", uploadedBytes: 4 })], activeCount: 1 });
    rerender(<UploadProgressDock />);
    expect(liveRegion).toHaveTextContent("archive.zip: Uploading");
    expect(liveRegion).not.toHaveTextContent("40%");

    current = manager();
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toBeEmptyDOMElement());
  });

  it("clears a removed transfer before announcing the same file again", async () => {
    const remainingTask = task({ id: "upload-2", name: "notes.zip" });
    let current = manager({
      tasks: [task({ status: "failed" }), remainingTask],
      activeCount: 1,
      failedCount: 1
    });
    mocks.useUploadManager.mockImplementation(() => current);
    const { rerender } = render(<UploadProgressDock />);
    const liveRegion = screen.getByRole("status");

    await waitFor(() => expect(liveRegion).toHaveTextContent("archive.zip: Upload failed"));

    current = manager({ tasks: [remainingTask], activeCount: 1 });
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toBeEmptyDOMElement());

    current = manager({
      tasks: [remainingTask, task({ id: "upload-3", status: "failed" })],
      activeCount: 1,
      failedCount: 1
    });
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toHaveTextContent("archive.zip: Upload failed"));
  });

  it("re-announces identical text when a transfer is replaced in one render", async () => {
    const remainingTask = task({ id: "upload-2", name: "notes.zip" });
    let current = manager({ tasks: [remainingTask], activeCount: 1 });
    mocks.useUploadManager.mockImplementation(() => current);
    const { rerender } = render(<UploadProgressDock />);
    const liveRegion = screen.getByRole("status");

    current = manager({
      tasks: [remainingTask, task({ status: "failed" })],
      activeCount: 1,
      failedCount: 1
    });
    rerender(<UploadProgressDock />);
    await waitFor(() => expect(liveRegion).toHaveTextContent("archive.zip: Upload failed"));
    const firstMessage = liveRegion.firstElementChild;

    current = manager({
      tasks: [remainingTask, task({ id: "upload-3", status: "failed" })],
      activeCount: 1,
      failedCount: 1
    });
    rerender(<UploadProgressDock />);

    await waitFor(() => expect(liveRegion.firstElementChild).not.toBe(firstMessage));
    expect(liveRegion).toHaveTextContent("archive.zip: Upload failed");
  });

  it("shows each transfer with its captured destination and progress", async () => {
    const cancelUpload = vi.fn();
    mocks.useUploadManager.mockReturnValue(manager({
      tasks: [
        task({ uploadedBytes: 4 }),
        task({ id: "upload-2", name: "assets.tar", destinationPath: "/Assets", file: new File(["0123456789"], "assets.tar") })
      ],
      activeCount: 2,
      queuedCount: 0,
      cancelUpload
    }));

    render(<UploadProgressDock />);

    expect(screen.getByText("2 active")).toBeInTheDocument();
    expect(screen.getByText("Daily/Reports")).toBeInTheDocument();
    expect(screen.getByText("Daily/Assets")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "archive.zip upload progress" })).toHaveAttribute("aria-valuenow", "40");
    expect(screen.getByRole("status")).toHaveTextContent("archive.zip: Uploading");
    expect(screen.getByRole("status")).not.toHaveTextContent("40%");

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
    expect(screen.getByRole("status")).toHaveTextContent("archive.zip: Upload failed");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel upload archive.zip" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Retry upload archive.zip" }));
    await userEvent.click(screen.getByRole("button", { name: "Dismiss upload archive.zip" }));
    expect(retryUpload).toHaveBeenCalledWith("upload-1");
    expect(dismissUpload).toHaveBeenCalledWith("upload-1");
  });

  it("shows queued uploads as cancelable without a progress bar", async () => {
    const cancelUpload = vi.fn();
    mocks.useUploadManager.mockReturnValue(manager({
      tasks: [task({ status: "queued" })],
      queuedCount: 1,
      cancelUpload
    }));

    render(<UploadProgressDock />);

    expect(screen.getByText("Queued")).toBeInTheDocument();
    expect(screen.getByText("1 queued")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Cancel upload archive.zip" }));
    expect(cancelUpload).toHaveBeenCalledWith("upload-1");
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
    queuedCount: 0,
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
