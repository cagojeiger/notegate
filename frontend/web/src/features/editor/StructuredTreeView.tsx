import { collapseAllNested, JsonView, type Props as JsonViewProps } from "react-json-view-lite";

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

export function StructuredTreeView({ value }: { value: Record<string, unknown> | unknown[] }) {
  return (
    <JsonView
      data={value}
      style={jsonTreeStyle}
      shouldExpandNode={collapseAllNested}
      clickToExpandNode
      compactTopLevel={Array.isArray(value) ? false : true}
    />
  );
}
