import { compareRevisions } from "./revisionDiff";

self.onmessage = (event: MessageEvent<unknown>) => {
  // A dedicated Worker's implicit parent port leaves origin empty and source null.
  // This is a private Worker channel, not a Window postMessage receiver.
  // https://html.spec.whatwg.org/multipage/web-messaging.html#message-port-post-message-steps
  if (event.origin !== "" || event.source !== null) return;
  const data = event.data;
  if (!data || typeof data !== "object" || !("before" in data) || !("after" in data)
    || typeof data.before !== "string" || typeof data.after !== "string") return;
  self.postMessage(compareRevisions(data.before, data.after));
};
