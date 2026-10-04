# Text revisions

Text revisions preserve recoverable bodies separately from audit events and live-text usage. The initial API supports plain and server-encrypted Text. Client-encrypted payloads are not recorded or decoded; history is unavailable while the current document uses client encryption. Binary attachments are out of scope.

## Save and editing groups

`write`, `append`, `patch`, and `edit` converge on `save_text_content`. After the existing checks, one transaction reserves history capacity, snapshots the old body, advances body attribution/group metadata, updates current content and records the existing file-change event. Any failure rolls back all of these. Unchanged or conflicting saves add no history.

Existing documents are backfilled with their last known author/time; pre-feature overwritten bodies cannot be recovered. New documents start with an independent initial state. Copying creates independent history; rename/move and encryption-policy changes create no body revision. Body attribution is stored separately from metadata `updated_at`.

REST v1/v2 text mutations and the direct command/MCP `write` input accept optional `edit_session_id` (UUID). Clients must use a new ID for a new editing session or AI operation. This identifier is a grouping hint, never an authorization credential. The server also requires the same document, authenticated account and transport channel. Backend callers with no channel use `unknown`.

- Missing session ID: every changed save is independent. Existing clients remain compatible.
- Same actor, channel and ID: continue only while the last content save is less than 120 seconds old and the group is less than 600 seconds old.
- At either boundary, or when actor/channel/ID changes: start a new group, even if an old ID is reused.
- Restore: independent group, never coalesced into ordinary edits.
- Sequence commands currently omit session IDs and remain independent. No frontend change is included in this implementation.

For `A -> B -> C -> D` in one editing group followed by another group's `D -> E -> F`, keep the initial `A`, boundary `D`, and final `F`; `B`, `C`, and `E` are intermediate states. The last state stays in `text_objects` until it is replaced. At replacement, whether the old state is a checkpoint is recorded permanently, so repeated cleanup cannot merge formerly separate groups. This is snapshot selection, not text merging or diff compression.

## Retention and capacity

Policy constants live together in `backend/crates/db/src/files/revisions.rs`:

- Every historical state is protected for at least 24 hours **after replacement**, even if its original content was written months ago.
- Intermediate states become eligible for deletion after 24 hours; checkpoints after 30 days, both measured from replacement.
- These are cleanup eligibility times, not exact deletion deadlines. Current content is never a cleanup target.
- There is no last-N cap that can silently truncate the protected 24-hour window.
- Each Space has a separate 1 GiB history-body budget. `text_revision_usage.stored_bytes` counts ciphertext plus nonce bytes, including soft-deleted documents. This is not physical database size: table/index overhead and backups are additional.
- When a changed save would exceed that budget, return `409` and leave current content/history unchanged. Do not silently delete protected revisions or save without history. Identical saves remain no-ops.
- Successful expiration/hard deletion releases the budget transactionally. Soft deletion hides history but retains it until normal expiration or document purge.

The budget is intentionally separate from tier-dependent live text/file quotas and their existing recalculation. Operators can inspect `text_revision_usage` and compare it with `SUM(text_revisions.stored_bytes)` per Space. History bytes are not yet added to the frontend usage display.

## History API and restore

Browser endpoints (under `/api`):

- `GET /v1/spaces/{space_id}/text/{node_id}/revisions?limit=50&cursor=...`
- `GET /v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}`
- `POST /v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}/restore` with required `expected_sha256` of the current document.

Lists return metadata only, newest first, with a signed document-scoped cursor; limit is 1–100. A selected body is loaded/decrypted separately. Current content is obtained through the existing Text read API. An expired and already deleted revision returns 404.

The service checks current Space permission, document visibility and external-access policy for every call. A revision ID does not bypass document/Space scoping. Restore requires write permission and uses the existing guarded write path, including current write locks, format validation, quotas and encryption policy. A stale current hash returns 409. Restoring identical content is a no-op; otherwise the replaced current body is preserved. Restore does not rewind or erase history. Public v2/MCP history browsing tools and frontend UI are later integrations; all existing mutation surfaces already record history.

## Encryption

All historical bodies, including those of otherwise plaintext documents, are AES-GCM encrypted with the configured server key. Authentication data binds Space, document and revision ID, preventing ciphertext from being substituted across revisions. Lists and cleanup do not decrypt bodies. Turning current-document encryption on/off never leaves plaintext history behind and does not change historical attribution.

This reuses the existing single configured encryption-key model; it does not add key rotation or an old-key fallback. A key mismatch fails closed. Encryption-root replacement must account for historical bodies as well as existing encrypted data; simply changing/removing the key will make them unreadable. Backups must preserve the required key through their own retention window.

## Reconciliation

One `text_revisions.retention` kind uses the existing reconciliation runtime, schedule, advisory lock and metrics. No new queue, worker, crate, process or per-document timer is created.

Every ten minutes, process at most 100 eligible rows from one live Space in one transaction, using the existing Space mutation lock order. Indexes support due-time selection and per-Space cleanup. If rows were deleted, release the runtime lock and request a follow-up after one second. Lock acquisition is bounded to two seconds; failure/timeout retries on the next normal schedule. Space purge owns cascades for deleted Spaces.

A cleanup failure retains extra history rather than losing a checkpoint. It can delay capacity recovery; existing reconciliation outcome/duration/last-success metrics and `text_revisions.cleaned` counts provide operational evidence. Deletion triggers release history usage, including ordinary resource cascades.

## Validation

CI exercises atomic rollback, no-op and competing writes, group boundaries, recent protection of old current content, repeated cleanup, expiration, quota accounting, cascade deletion, encrypted identity binding, access controls, write locks, encryption transitions, pagination and guarded restore. Local builds/tests are not required for this change.
