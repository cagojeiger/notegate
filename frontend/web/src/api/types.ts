export type Page = {
  limit: number;
  count: number;
  has_more: boolean;
  next_cursor: string | null;
};

export type Me = {
  account: {
    id: string;
    kind: "user" | "agent";
    display_name: string;
  };
  user?: {
    sub: string;
    email: string;
  } | null;
  capabilities?: Record<string, boolean>;
};

export type Space = {
  id: string;
  name: string;
  sort_order: number;
  permission: string;
  root_node_id: string;
  created_at: string;
  updated_at: string;
};

export type NodeKind = "folder" | "text" | "file";

export type AccountRef = {
  id: string;
  kind: "user" | "agent";
  display_name: string;
};

export type RestNode = {
  id: string;
  space_id: string;
  parent_id: string | null;
  name: string;
  kind: NodeKind;
  path: string;
  sort_order: number;
  metadata: Record<string, unknown>;
  has_children: boolean;
  content_sha256?: string;
  byte_len?: number;
  line_count?: number;
  storage_kind?: "inline_pg" | "object";
  media_type?: string;
  original_filename?: string;
  encryption_mode?: "none" | "client";
  encryption_metadata?: Record<string, unknown>;
  created_by: AccountRef;
  updated_by: AccountRef;
  created_at: string;
  updated_at: string;
};

export type SpacesListResponse = {
  spaces: Space[];
  page: Page;
};

export type RestNodeListResponse = {
  nodes: RestNode[];
  page: Page;
};

export type ChildrenResponse = {
  parent: {
    id: string;
    path: string;
  };
  children: RestNode[];
  page: Page;
};

export type NodeRevealResponse = {
  ancestors: RestNode[];
  target: RestNode;
};
