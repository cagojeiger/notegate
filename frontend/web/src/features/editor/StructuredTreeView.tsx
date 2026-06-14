import { allExpanded, collapseAllNested, JsonView, type Props as JsonViewProps } from "react-json-view-lite";

export type StructuredExpansionMode = "default" | "expanded" | "collapsed";

const jsonTreeStyle: NonNullable<JsonViewProps["style"]> = {
  container: "ng-json-tree",
  childFieldsContainer: "ng-json-tree-children",
  basicChildStyle: "ng-json-tree-child",
  collapseIcon: "ng-json-tree-collapse",
  expandIcon: "ng-json-tree-expand",
  collapsedContent: "ng-json-tree-collapsed",
  label: "ng-json-tree-label",
  clickableLabel: "ng-json-tree-label ng-json-tree-label-clickable",
  nullValue: "ng-json-tree-null",
  undefinedValue: "ng-json-tree-null",
  numberValue: "ng-json-tree-number",
  stringValue: "ng-json-tree-string",
  booleanValue: "ng-json-tree-boolean",
  otherValue: "ng-json-tree-other",
  punctuation: "ng-json-tree-punctuation",
  quotesForFieldNames: false,
  noQuotesForStringValues: false,
  stringifyStringValues: true,
  ariaLables: {
    collapseJson: "Collapse JSON node",
    expandJson: "Expand JSON node"
  }
};

function shouldExpandNode(mode: StructuredExpansionMode) {
  if (mode === "expanded") return allExpanded;
  if (mode === "collapsed") return () => false;
  return collapseAllNested;
}

export function StructuredTreeView({ value, expansionMode = "default" }: { value: Record<string, unknown> | unknown[]; expansionMode?: StructuredExpansionMode }) {
  return (
    <JsonView
      key={expansionMode}
      data={value}
      style={jsonTreeStyle}
      shouldExpandNode={shouldExpandNode(expansionMode)}
      clickToExpandNode
      compactTopLevel={Array.isArray(value) ? false : true}
      aria-label="Structured data tree"
    />
  );
}
