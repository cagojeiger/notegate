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
