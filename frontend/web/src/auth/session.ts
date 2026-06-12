const API_KEY_STORAGE_KEY = "notegate.devApiKey";

export function readDevApiKey(): string | null {
  return window.sessionStorage.getItem(API_KEY_STORAGE_KEY);
}

export function writeDevApiKey(apiKey: string): void {
  window.sessionStorage.setItem(API_KEY_STORAGE_KEY, apiKey);
}

export function clearDevApiKey(): void {
  window.sessionStorage.removeItem(API_KEY_STORAGE_KEY);
}
