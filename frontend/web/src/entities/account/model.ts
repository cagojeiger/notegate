export type AccountRef = {
  id: string;
  kind: "user" | "agent";
  display_name: string;
};

export type Me = {
  account: AccountRef;
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
