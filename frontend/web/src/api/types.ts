export type Page = {
  limit: number;
  returned: number;
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
    email?: string | null;
  } | null;
  agent?: {
    name: string;
  } | null;
  capabilities: {
    can_create_space: boolean;
    can_manage_agents: boolean;
  };
};

export type SpacePermission = "read" | "write";

export type Space = {
  id: string;
  name: string;
  sort_order: number;
  permission: SpacePermission;
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
  media_type?: string;
  detected_media_type?: string;
  preview_available?: boolean;
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

export type ReadTextResponse = {
  node: {
    id: string;
    path: string;
  };
  text:
    | {
        node_id: string;
        storage_format: "plain";
        content: string;
        content_sha256: string;
        byte_len: number;
        line_count: number;
        start_line: number;
        end_line: number;
        returned_lines: number;
        truncated: boolean;
        next_start_line: number | null;
        updated_by: AccountRef;
        updated_at: string;
      }
    | {
        node_id: string;
        storage_format: "encrypted";
        encrypted_payload: unknown;
        content_sha256: string;
        byte_len: number;
        line_count: number;
        updated_by: AccountRef;
        updated_at: string;
      }
    | {
        node_id: string;
        storage_format: string;
        unchanged: boolean;
        content_returned: boolean;
        content_sha256: string;
        byte_len: number;
        line_count: number;
      };
};

export type TextResponse = {
  node: {
    id: string;
    path: string;
  };
  text: {
    node_id: string;
    storage_format: string;
    content_sha256: string;
    byte_len: number;
    line_count: number;
    updated_by: AccountRef;
    updated_at: string;
  };
};

export type FileResponse = {
  node: RestNode;
};

export type FilePreviewUrlResponse = {
  url: string;
  media_type: string;
  expires_at: string;
};

export type BatchFilePreviewItem = {
  path: string;
  status: "ready" | "not_found" | "unsupported" | "error";
  node_id: string | null;
  media_type: string | null;
  url: string | null;
  expires_at: string | null;
};

export type BatchFilePreviewResponse = {
  results: BatchFilePreviewItem[];
};

export type BeginFileUploadResponse = {
  upload_id: string;
  transfer: SingleFileUploadTransfer | MultipartFileUploadTransfer;
};

export type SingleFileUploadTransfer = {
  mode: "single";
  url: string;
  headers: Record<string, string>;
};

export type MultipartFileUploadTransfer = {
  mode: "multipart";
  part_size: number;
  part_count: number;
};

export type PreparedFileUploadPart = {
  part_number: number;
  url: string;
  headers: Record<string, string>;
  content_length: number;
};

export type CompletedFileUploadPart = {
  part_number: number;
  etag: string;
};

export type MetadataResponse = {
  metadata: Record<string, unknown>;
};

export type AuditEvent = {
  id: number;
  created_at: string;
  actor_account_id: string | null;
  actor?: AccountRef | null;
  source: string;
  op_type: string;
  resource_type: string;
  resource_id: string | null;
  metadata: Record<string, unknown>;
};

export type AuditEventListResponse = {
  events: AuditEvent[];
  page: Page;
};

export type FileChangeEvent = {
  id: number;
  created_at: string;
  space_id: string;
  node_id: string | null;
  actor_account_id: string | null;
  actor?: AccountRef | null;
  op_type: string;
  metadata: Record<string, unknown>;
};

export type FileChangeEventListResponse = {
  events: FileChangeEvent[];
  page: Page;
};

export type FileChangeDelta = {
  id: number;
  node_id: string | null;
  op_type: string;
  item_kind: RestNode["kind"] | null;
  affected_parent_ids: string[];
  parent_scope_known: boolean;
  path_changed: boolean;
  subtree_changed: boolean;
};

export type FileChangeSyncResponse = {
  changes: FileChangeDelta[];
  next_after_id: number;
  has_more: boolean;
  resync_required: boolean;
};
