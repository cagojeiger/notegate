# Text revisions

Backend implementation in progress. Text revisions are separate from audit events and live text usage.

- A changed, successful save preserves the previous body in the same transaction. Identical saves do not add a revision.
- History bodies are always encrypted with the existing server key and revision-bound authentication data, including history of plaintext documents. Client-encrypted documents are outside the initial history API scope.
- Group only authenticated actor/channel plus an explicit editing-session or operation ID. Missing IDs mean independent saves. Split after two minutes idle or ten minutes from group start; restore is an independent boundary.
- Protect every historical state for at least 24 hours after replacement. Afterwards remove only intermediate states; retain states before and after each editing group. Overall retention and history byte budget are explicit policies, separate from live-content quotas.
- List metadata with bounded cursor pagination, fetch one historical body on demand, and restore via the normal guarded write path. Current permission, external-access, deletion and write-lock rules still apply.
- One existing-runtime reconciler performs bounded, repeatable cleanup. No new worker, queue, crate, or process.
- Frontend integration is a later change. Old clients still create independent history records.

Validation runs in CI: atomic rollback, no-op/conflicting writes, authorization, encryption, grouping/time boundaries, cleanup replay, quota accounting and cascade deletion.
