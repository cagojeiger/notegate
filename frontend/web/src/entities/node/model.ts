import type { AccountRef } from "../account/model";

export type NodeKind = "folder" | "text" | "file";

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
