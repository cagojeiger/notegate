import type { RestNode } from "../../api/types";
import { Card, MetaRow } from "../../shared/ui";
import { useFolderChildrenStat } from "./useEditorQueries";

export function FolderDetailView({ node }: { node: RestNode }) {
  const childrenQuery = useFolderChildrenStat(node);
  const childCount = childrenQuery.data ? `${childrenQuery.data.children.length}${childrenQuery.data.page.has_more ? "+" : ""}` : "…";
  return (
    <article className="mx-auto max-w-3xl px-10 py-14">
      <p className="text-sm text-muted">{node.path}</p>
      <h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1>
      <Card className="mt-8">
        <dl className="space-y-3">
          <MetaRow label="Children" value={childCount} />
          <MetaRow label="Updated" value={node.updated_at.slice(0, 10)} />
        </dl>
      </Card>
      <p className="mt-8 leading-7 text-muted">Folder selected. Use the tree to browse children or create a node.</p>
    </article>
  );
}
