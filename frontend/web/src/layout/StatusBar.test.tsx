import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { StatusBar } from "./StatusBar";

describe("StatusBar", () => {
  it("opens the transfer view from the runtime upload indicator", async () => {
    const user = userEvent.setup();
    const onOpenTransfers = vi.fn();

    render(
      <StatusBar
        activeSpace={null}
        activeUploads={2}
        failedUploads={1}
        uploadProgress={35}
        onOpenTransfers={onOpenTransfers}
      />
    );

    expect(screen.getByText("2 uploading · 35% · 1 failed")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open file transfers" }));
    expect(onOpenTransfers).toHaveBeenCalledTimes(1);
  });
});
