import { createApiClient, type ApiClient } from "../api/client";
import type { LoginFlow } from "../auth/loginFlow";

export type AppRuntime = {
  createApiClient(): ApiClient;
  loginFlow?: LoginFlow;
};

export const browserAppRuntime: AppRuntime = {
  createApiClient
};
