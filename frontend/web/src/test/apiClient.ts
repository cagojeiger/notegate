import { vi, type Mock } from "vitest";

import type { ApiClient } from "../api/client";

type ApiMethod = (...args: never[]) => unknown;
type MockApiMethod<Method extends ApiMethod> = Method & Mock<
  (...args: Parameters<Method>) => ReturnType<Method>
>;

type MockApiClient = {
  [Method in keyof ApiClient]: MockApiMethod<ApiClient[Method]>;
};

export function createMockApiClient(): MockApiClient {
  return {
    get: createApiMethod<ApiClient["get"]>(),
    post: createApiMethod<ApiClient["post"]>(),
    put: createApiMethod<ApiClient["put"]>(),
    patch: createApiMethod<ApiClient["patch"]>(),
    delete: createApiMethod<ApiClient["delete"]>(),
    download: createApiMethod<ApiClient["download"]>()
  };
}

function createApiMethod<Method extends ApiMethod>(): MockApiMethod<Method> {
  return vi.fn<
    (...args: Parameters<Method>) => ReturnType<Method>
  >() as MockApiMethod<Method>;
}
