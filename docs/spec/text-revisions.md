# Text revisions

Text revisions preserve recoverable bodies separately from audit events and live-text usage. The API supports plain and server-encrypted Text. Client-encrypted payloads are not recorded or decoded; history is unavailable while the current document uses client encryption. Binary attachments are out of scope.

## Save and editing groups

`write`, `append`, `patch`, and `edit` converge on `save_text_content`. After validation, one transaction reserves history capacity, snapshots the old body, updates current content and body attribution/group metadata in one statement, and records the file-change event. Any failure rolls back all of these. Unchanged or conflicting saves add no history.

Only recorded revision bodies can be restored. New documents start with an independent initial state. Copying creates independent history; rename/move and encryption-policy changes create no body revision. Body attribution is stored separately from metadata `updated_at`.

MCP and CLI write commands require a top-level `purpose` (at most 200 characters). The shared command executor records it on the saved body and invocation log. Creation records its purpose immediately; subsequent changed writes atomically carry the old body's purpose into its historical snapshot and record the new purpose on the current body. Failed/conflicting and unchanged saves do not replace the saved purpose. Sequence writes inherit their top-level purpose. Bodies without a recorded purpose show no reason; reasons are never reconstructed by guessing from timestamps or paths. Restore starts a new body with no inherited AI purpose. These caller-supplied reasons describe the declared intent, not independently verified reasoning.

REST v1/v2 text mutations and command/MCP `write` inputs (direct and sequence) accept optional `edit_session_id` (UUID). Clients must use a new ID for a new editing session or AI operation. This identifier is a grouping hint, never an authorization credential. The server also requires the same document, authenticated account and transport channel. Backend callers with no channel use `unknown`.

- Missing session ID: every changed save is independent.
- Same actor, channel and ID: continue only while the last content save is less than 120 seconds old and the group is less than 600 seconds old.
- At either boundary, or when actor/channel/ID changes: start a new group, even if an old ID is reused.
- Restore: independent group, never coalesced into ordinary edits.
- Sequence write commands support the same optional ID and validate it during preflight, before executing earlier writes. The web editor retains an editing-session ID for retries of the current edit; a successful save, cancel, document switch, and restore reset it. Re-entering Edit after saving starts an independent group. Backend idle/group limits still apply.

For `A -> B -> C -> D` in one editing group followed by another group's `D -> E -> F`, keep the initial `A`, boundary `D`, and final `F`; `B`, `C`, and `E` are intermediate states. The last state stays in `text_objects` until it is replaced. At replacement, whether the old state is a checkpoint is recorded permanently, so repeated cleanup cannot merge formerly separate groups. This is snapshot selection, not text merging or diff compression.

## Retention and capacity

Policy constants live together in `backend/crates/db/src/files/revisions.rs`:

- Every historical state is protected for at least 24 hours **after replacement**, even if its original content was written months ago.
- Intermediate states become eligible for deletion after 24 hours; checkpoints after 30 days, both measured from replacement.
- These are cleanup eligibility times, not exact deletion deadlines. Current content is never a cleanup target.
- There is no last-N cap that can silently truncate the protected 24-hour window.
- Each Space has a separate 1 GiB history-body budget. `text_revision_usage.stored_bytes` counts ciphertext plus nonce bytes, including soft-deleted documents. This is not physical database size: table/index overhead and backups are additional.
- When a changed save would exceed that budget, return `422` (`text_revision_storage_full`) and leave current content/history unchanged. Do not silently delete protected revisions or save without history. Identical saves remain no-ops.
- Successful expiration/hard deletion releases the budget transactionally. Soft deletion of a document, folder or Space hides history without pausing its normal expiration. Trash restoration exposes only revisions that still remain.

Changes retains revision IDs independently of historical bodies. Revision expiration does not remove the current document or its Changes, and missing historical bodies do not prevent trash restoration when current content remains available. Restoring a selected revision requires reading that revision's body; an already removed body returns 404 without changing the current document.

The budget is separate from tier-dependent text/file quotas. Existing Space usage reconciliation also repairs `text_revision_usage` from `SUM(text_revisions.stored_bytes)`, including trashed documents, under the Space gate and counter lock. It repairs accounting without recreating missing bodies or inventing deletion receipts. It is an explicit reconciliation path, not a continuous detector of external database edits. The frontend usage display excludes history bytes.

## History API and restore

Browser endpoints (under `/api`):

- `GET /v1/spaces/{space_id}/text/{node_id}/revisions?limit=50&cursor=...`
- `GET /v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}`
- `POST /v1/spaces/{space_id}/text/{node_id}/revisions/{revision_id}/restore` with required `expected_sha256` of the current document.

Lists return metadata only, newest first, with the shared `page` object (`limit`, `returned`, `has_more`, `next_cursor`) and a signed document-scoped cursor; limit is 1–100. A selected body is loaded/decrypted separately. Current content is obtained through the existing Text read API. An expired and already deleted revision returns 404.

Revision metadata includes nullable `purpose`. Lists also return nullable `current` metadata (`content_sha256`, `purpose`) without reading or decrypting a body. The web modal displays the selected version's change reason, and shows the current reason only when its hash matches the comparison baseline. Missing reasons are displayed as `Not recorded`. Purpose is bounded, caller-supplied metadata under the same current document/Space access checks; it is encrypted separately from the body, bound to Space, document and revision IDs. Lists decrypt only this small metadata field. New writes store no plaintext purpose. The history privacy reconciler migrates legacy current and historical reasons in batches of at most 100 rows per table. Rolling-deployment triggers preserve encrypted reasons when an older writer archives a body; an old reader may temporarily show a missing reason until it is upgraded.

The web comparison shows cumulative differences from the selected version to the current saved body, not the individual edit that created the selected version. Its save reason is labelled separately and can be expanded. Version timestamps include seconds. Comparisons initially reveal the first changed line, show added/removed line counts, and retain three context lines around each change; overlapping context forms one navigable section. Unchanged gaps can be expanded without another API call. Previous/next controls scroll only the comparison, and manual scrolling updates the active change indicator. On narrow screens the version list collapses into a picker to preserve reading space; selecting a version closes the list and returns focus to the picker. Identical bodies show an explicit no-changes state. Restore always applies the entire selected version, including collapsed content.

The service checks current Space permission, document visibility and external-access policy for every call. A revision ID does not bypass document/Space scoping. Restore requires write permission and uses the existing guarded write path, including current write locks, format validation, quotas and encryption policy. A stale current hash returns 409. Restoring identical content is a no-op; otherwise the replaced current body is preserved. Restore does not rewind or erase history. History list/read/restore are Browser V1 endpoints; Public V2 and MCP expose no history browsing tools. All text mutation surfaces record revisions.

## Encryption

All historical bodies, including those of otherwise plaintext documents, are AES-GCM encrypted with the configured server key. Authentication data binds Space, document and revision ID, preventing ciphertext from being substituted across revisions. Lists and cleanup do not decrypt bodies. Turning current-document encryption on/off never leaves plaintext history behind and does not change historical attribution.

Revision bodies use the configured encryption key without key rotation or an old-key fallback. A key mismatch fails closed. Encryption-root replacement must account for historical bodies as well as existing encrypted data; simply changing/removing the key will make them unreadable. Backups must preserve the required key through their own retention window.

## Reconciliation

The `text_revisions.retention` kind runs in the shared reconciliation runtime with its schedule, advisory lock and metrics.

Every ten minutes, process at most 100 eligible rows from one Space, including a soft-deleted Space, in one transaction. Retention acquires the existing shared Space gate and then the Space row lock; it does not require a live Space. This keeps it mutually exclusive with resource purge's exclusive gate without changing ordinary write authorization. A Space removed after candidate selection is a no-op. Indexes support due-time selection and per-Space cleanup. If rows were deleted, release the runtime lock and request a follow-up after one second. Row-lock waits are bounded to two seconds; failure/timeout retries on the next normal schedule. Irreversible resource purge removes remaining revisions in batches before deleting their document or Space.

A cleanup failure retains extra history rather than losing a checkpoint. It can delay capacity recovery; existing reconciliation outcome/duration/last-success metrics and `text_revisions.cleaned` counts provide operational evidence. Deletion triggers release history usage, including revision batches drained by resource purge.

AFTER DELETE triggers on `text_objects` and `text_revisions` record identifier-only `text_revision.delete` Audit receipts in the same transaction as body removal and usage release. The current body's receipt preserves its `revision_id`; ordinary save UPDATEs and archival do not create deletion receipts. Receipts use the actual database deletion time and retain for 180 days; cleanup's test/policy cutoff is not a claimed deletion timestamp. Retention supplies an explicit context, distinguishing intermediate/checkpoint expiration for historical bodies only; resource purge supplies its own context for both tables. Ordinary SQL and FK cascades without that context record `unknown`, even if the body was old. Transaction-local context cannot leak to another transaction. The retained accounting scope preserves receipt ownership after Space removal. Migrations install capture for subsequent DELETEs; they do not reconstruct receipts for previously missing bodies.

Changes checks actual body presence before interpreting receipts. Missing bodies without receipts remain unavailable with unknown cause; a receipt alongside an existing body is a visible inconsistency, not grounds for hiding/deleting the body. Query/authorization/decryption failures are not deletion evidence. Row triggers do not cover TRUNCATE, disabled triggers, or administrator tampering. Existing deadlines remain stored in `cleanup_at`; changing policy constants alone does not rewrite them.

## Validation

The database is the revision policy clock in production. Creation and each changed save sample it once after acquiring the write locks; cleanup samples one cutoff shared by candidate selection and deletion. The `test-util` feature exposes fixed-time injection for the same queries, without changing the database clock or accepting a client timestamp.

CI checks one microsecond before, exactly at, and one microsecond after the 120-second idle, 600-second group, 24-hour intermediate and 30-day checkpoint boundaries. Continued writes isolate the group-age limit from the idle limit. Tests also verify replacement-based retention, unchanged current content during cleanup, transactional usage accounting, and that no-op, hash-conflict, quota-rejected and rolled-back writes do not refresh an editing group.

CI exercises atomic rollback, no-op and competing writes, group boundaries, recent protection of old current content, repeated cleanup, expiration, quota accounting, cascade deletion, encrypted identity binding, access controls, write locks, encryption transitions, pagination and guarded restore.

Trash-retention tests cover document, folder and Space deletion, the exact intermediate expiry boundary, current-body preservation and restoration of remaining checkpoints. A database-lock-controlled test runs retention and Space purge in both orders and verifies retry behavior, single byte release and one receipt per deleted revision.

## Write cost and CI comparison

Full replacement loads node and text metrics for preflight, without fetching/decrypting the previous body. The transaction still locks and loads that body to preserve the recoverable snapshot. Append, patch and edit retain their required content reads. Changed saves update current content and revision attribution together once; a trigger-based regression test checks the actual row-update count.

Account-deletion safety locks the active Space owner's account before the Space. Concurrent writes to different Spaces owned by the same user can therefore serialize.

The `Text Write Performance` workflow compares a PR's base and candidate on one Linux runner and PostgreSQL 17 using the same ignored service benchmark. It builds the release profile once per revision, then alternates three paired trials. Cases cover 10 KiB, 100 KiB and 1 MiB bodies, plain/server encryption, sequential saves, four writers in one Space, different Spaces under one owner, and different owners. Each writer uses a separate document and performs two warmup saves followed by 20 changed saves through a four-connection pool. An untimed CHECKPOINT after warmup starts each case at the same checkpoint phase; durability remains enabled. This avoids carryover from earlier cases triggering WAL checkpoints in unrelated samples. History bodies/counts and usage are verified after timing. The summary and raw measurements are retained as `text-write-performance`.

Measurements are service-call p95 and throughput for synthetic repeated-character bodies; they include pool/row-lock waits and crypto, but exclude HTTP middleware, production networking, real workload distributions and long-running cleanup. Reported figures are medians across three trial percentiles/rates. Timing is informational and has no noisy CI pass/fail threshold; compilation, fixture correctness and complete measurement coverage must pass.

## Deployment compatibility

- Every active Text writer must support revision recording and body attribution. Do not run or roll back to writers that bypass these guarantees; a mutable tag or overlapping rollout alone does not prove compatibility.
- Preserve the configured encryption root/key ID and recovery keys for retained revisions and backups.
- `api`/`all` applies migrations; other roles start after schema readiness. Do not edit or reapply recorded migration checksums. Schema recovery follows the [database deployment contract](db.md#deployment-and-rollback).
- Validate a controlled document's guarded save, revision read/restore, usage consistency and retention reconciliation. These operations create real revisions.
- Monitor write latency/errors, DB-pool acquisition waits/timeouts, lock waits, history capacity and `text_revisions.retention` results.
- Release digest, GitOps desired digest and running Pod `imageID` are separate deployment evidence.

## Web version history

The document header has a Version history button between Edit and More actions. The same action is available in the editor menu, including narrow screens. It opens the shared modal shell; no Inspector tab is added. The editor stays mounted so opening and closing history preserves unsaved drafts.

The modal loads metadata in 50-row pages and one selected body on demand. It compares a selected historical body with a separately fetched, stable current saved body. Comparison never uses an unsaved draft. Full version reuses existing format previews. Preview links do not navigate the active window, preserving any unsaved editor draft; users can copy a link address separately. Restore needs write permission, an unlocked document, no dirty draft or pending save, and a confirmation. After a successful restore, cancel older in-flight editor reads and publish the selected body and returned hash to the canonical text cache before closing the modal. Background reads refresh attribution and other metadata without replacing a new draft. A 409 asks the user to reload and review; it is never retried with a fresh hash automatically. A 422 `text_revision_storage_full` is a capacity error, not an overwrite prompt.

Line comparison runs in a disposable module Worker with a two-second deadline, at most 256,000 UTF-16 code units, 1,500 combined source lines and 1,000,000 LCS cells. Exceeding a bound or Worker failure leaves Full version available. Wide screens align old and current source side by side; narrow screens show a unified view with explicit addition/removal markers. Historical bodies and comparison baselines are evicted when their query observers are gone. The modal uses no polling or server diff endpoint.

CI covers lossless diff reconstruction, changed/inserted lines, newline differences, input bounds, lazy history loading, guarded restore, draft preservation, read-only/mobile access and stale-hash recovery. Browser-generated desktop/mobile screenshots are retained as the `text-revisions-ui` CI artifact.
