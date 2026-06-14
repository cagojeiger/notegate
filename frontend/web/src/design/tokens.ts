export const themes = ["light", "dark"] as const;
export type ThemeMode = (typeof themes)[number];
