import { Plus } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";

import { Button } from "../../shared/ui";

export function SettingsAddButton({ children, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { children: ReactNode; ref?: Ref<HTMLButtonElement> }) {
  return <Button secondary className="h-12 w-full border-dashed" {...props}><Plus size={17} />{children}</Button>;
}
