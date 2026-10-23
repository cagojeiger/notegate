import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { useLayoutEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";

import type { RestNode, Space } from "../api/types";
import type { CurrentUserUsage } from "../api/usage";
import { useUiStore } from "../stores/uiStore";
import { makeRestNode, makeSpace } from "../test/fixtures";
import { App } from "./App";

const probes = vi.hoisted(() => ({
  clients: new Map<string, QueryClient>(),
  commits: [] as { space?: string; nodeSpace?: string; name?: string }[]
}));
vi.mock("../features/editor/EditorArea", () => ({
  EditorArea: function EditorProbe({ activeSpace, groups }: { activeSpace: Space | null; groups: { node: RestNode | null }[] }) {
    const client = useQueryClient();
    useLayoutEffect(() => {
      if (activeSpace) probes.clients.set(activeSpace.id, client);
      probes.commits.push({ space: activeSpace?.id, nodeSpace: groups[0]?.node?.space_id, name: groups[0]?.node?.name });
    });
    return <p>Editor {activeSpace?.id}: {groups[0]?.node?.name ?? "empty"}</p>;
  }
}));
vi.mock("../features/nodes/PrimarySidebar", () => ({ PrimarySidebar: () => null }));
vi.mock("../layout/AuxiliarySidebar", () => ({ AuxiliarySidebar: () => null }));
afterEach(() => useUiStore.setState(useUiStore.getInitialState(), true));

it("clears in-memory workbench selection before rendering a different authenticated owner's spaces", async () => {
  let owner = "a";
  const a = makeSpace({ id: "space-a", name: "Space A", root_node_id: "root-a" });
  const b = makeSpace({ id: "space-b", name: "Space B", root_node_id: "root-b" });
  const note = makeRestNode({ id: "note-a", space_id: a.id, parent_id: a.root_node_id, name: "Private owner A note" });
  useUiStore.setState({ ...useUiStore.getInitialState(), activeSpaceId: a.id,
    editorGroups: [{ id: 0, node: note, mode: "preview", back: [], forward: [] }], activeGroupIndex: 0 }, true);
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = new URL(String(input), "http://localhost");
    const space = owner === "a" ? a : b;
    let body: unknown;
    if (url.pathname === "/auth/session/activity") return new Response(null, { status: 204 });
    if (url.pathname === "/api/v1/me") body = {
      account: { id: owner, kind: "user", display_name: owner }, user: { email: `${owner}@example.invalid` },
      capabilities: { can_create_space: true, can_manage_agents: true }
    };
    else if (url.pathname === "/api/v1/spaces") body = { spaces: [space], page: { limit: 100, returned: 1, has_more: false, next_cursor: null } };
    else if (url.pathname === "/api/v1/me/usage") body = { tier: "max", spaces: [] } satisfies CurrentUserUsage;
    else if (url.pathname.endsWith("/file-change-sync")) body = { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    else throw new Error(`Unhandled ${url.pathname}`);
    return new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
  });
  render(<App />);
  await screen.findByText("Editor space-a: Private owner A note");
  const previous = probes.clients.get(a.id)!;
  owner = "b";
  await act(async () => { await previous.invalidateQueries({ queryKey: ["me"] }); });
  await screen.findByText("Editor space-b: empty");
  expect(probes.commits.some((row) => row.space === b.id && row.nodeSpace === a.id)).toBe(false);
});
