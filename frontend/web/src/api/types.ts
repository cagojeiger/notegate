export type Page<T> = {
  items: T[];
  next_cursor: string | null;
  has_more: boolean;
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
