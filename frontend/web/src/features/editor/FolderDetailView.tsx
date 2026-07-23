import type { RestNode } from "../../api/types";
import { Card, MetaRow } from "../../shared/ui";
import { useFolderChildrenStat } from "./useEditorQueries";

export function FolderDetailView({ node }: { node: RestNode }) {
  const childrenQuery = useFolderChildrenStat(node);
  const childCount = childrenQuery.data ? `${childrenQuery.data.children.length}${childrenQuery.data.page.has_more ? "+" : ""}` : "…";
  return (
    <article className="min-h-0 w-full flex-1 overflow-y-auto">
      <div className="mx-auto max-w-[44rem] px-6 py-10 sm:px-10 sm:py-14">
        <p className="text-sm text-muted">{node.path}</p>
        <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">{node.name}</h1>
        <Card className="mt-8">
          <dl className="space-y-3">
            <MetaRow label="Children" value={childCount} />
            <MetaRow label="Updated" value={node.updated_at.slice(0, 10)} />
          </dl>
        </Card>
        <p className="mt-8 leading-7 text-muted">Folder selected. Use the tree to browse children.</p>
      </div>
    </article>
  );
}
