import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";

import { makeSpace } from "../../test/fixtures";
import { EmptyEditor } from "./EmptyEditor";

it("opens the file chooser from the keyboard and delivers the selected file", async () => {
  const user = userEvent.setup();
  const onFileSelected = vi.fn();
  const { container } = render(<EmptyEditor activeSpace={makeSpace()} canWriteActiveSpace
    onCreateFolder={vi.fn()} onCreateText={vi.fn()} onRecordAudio={vi.fn()} onFileSelected={onFileSelected} />);
  const input = container.querySelector<HTMLInputElement>('input[type="file"]')!;
  const openChooser = vi.fn();
  input.addEventListener("click", openChooser);
  screen.getByRole("button", { name: "Upload file" }).focus();
  await user.keyboard("{Enter}");
  expect(openChooser).toHaveBeenCalledTimes(1);

  const file = new File(["note"], "note.txt", { type: "text/plain" });
  await user.upload(input, file);
  expect(onFileSelected).toHaveBeenCalledTimes(1);
  expect(onFileSelected).toHaveBeenCalledWith(file);
});
