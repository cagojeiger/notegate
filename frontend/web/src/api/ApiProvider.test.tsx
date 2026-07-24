import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useMutation } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { ApiProvider } from "./ApiProvider";

describe("ApiProvider", () => {
  it("delegates unhandled mutation errors to the application boundary", async () => {
    const onMutationError = vi.fn();

    render(
      <ApiProvider apiKey="test-key" authCacheKey="test-key:0" onMutationError={onMutationError}>
        <FailingMutation />
      </ApiProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Fail mutation" }));

    await waitFor(() => expect(onMutationError).toHaveBeenCalledWith("Mutation failed"));
  });
});

function FailingMutation() {
  const mutation = useMutation({
    mutationFn: async () => {
      throw new Error("Mutation failed");
    }
  });

  return <button onClick={() => mutation.mutate()}>Fail mutation</button>;
}
