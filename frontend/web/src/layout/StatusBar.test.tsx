import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatusBar } from "./StatusBar";

describe("StatusBar", () => {
  it("shows the save state and active space", () => {
    render(<StatusBar activeSpace={null} />);

    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getByText("No space")).toBeInTheDocument();
  });
});
