import type { ReactNode } from "react";

import { Card } from "./Card";

export function EmptyState({ children }: { children: ReactNode }) {
  return <Card className="text-sm text-muted">{children}</Card>;
}
