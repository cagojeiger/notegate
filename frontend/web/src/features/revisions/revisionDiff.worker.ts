import { compareRevisions } from "./revisionDiff";

self.onmessage = (event: MessageEvent<{ before: string; after: string }>) => {
  self.postMessage(compareRevisions(event.data.before, event.data.after));
};
