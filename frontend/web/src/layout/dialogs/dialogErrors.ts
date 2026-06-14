export function dialogErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Action failed";
}
