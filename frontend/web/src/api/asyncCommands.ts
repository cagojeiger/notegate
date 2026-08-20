import type { ApiClient } from "./client";
import { isApiRouteNotFound } from "./errors";
import type { AsyncCommandAck } from "./types";

type LegacyAsyncCommandAck = {
  status: "accepted" | "queued";
  job_id?: string;
};

function normalizeAsyncCommandAck(
  response: AsyncCommandAck | LegacyAsyncCommandAck
): AsyncCommandAck {
  return "result" in response
    ? response
    : {
      result: "accepted",
      availability: { can_trigger: false, reason: "pending", retry_at: null }
    };
}

export async function postAsyncCommand(
  client: ApiClient,
  path: string,
  previousPath: string
): Promise<AsyncCommandAck> {
  try {
    const response = await client.post<AsyncCommandAck | LegacyAsyncCommandAck>(path);
    return normalizeAsyncCommandAck(response);
  } catch (error) {
    if (!isApiRouteNotFound(error)) throw error;
    const response = await client.post<AsyncCommandAck | LegacyAsyncCommandAck>(previousPath);
    return normalizeAsyncCommandAck(response);
  }
}
