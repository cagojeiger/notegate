# 파일 DB 스키마

notegate는 개인 Markdown 파일시스템을 Postgres에 저장한다. DB는 정본 저장소이며,
REST와 MCP는 모두 이 구조 위에서 동작한다.

## 테이블 개요

정본 테이블:

```text
accounts          user/agent 공통 행위자 identity
users             authgate OAuth 사용자 상세
agents            AI agent 상세
agent_keys        agent API key credential hash
account_encryption_keys account별 PII DEK wrapping metadata
workspaces        개인 노트 workspace 경계
workspace_access  workspace membership과 owner/editor/viewer 권한
nodes             folder/document 공통 tree node
documents         markdown document 본문
```

현재 단계는 원본 Markdown 저장만 고려한다. grep은 `documents.content_md`를 `ILIKE`로
후보 검색하고 application code에서 line-split한다. 권한은 workspace 단위로만 적용하고,
file/folder/node 단위 ACL은 도입하지 않는다.

## accounts

`accounts`는 notegate에서 행동할 수 있는 공통 주체다. 사람 사용자와 AI agent 모두
하나의 `accounts.id`를 가진다.

```sql
CREATE TABLE accounts (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind                    TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name_ciphertext BYTEA,
    display_name_nonce      BYTEA,
    is_active               BOOLEAN NOT NULL DEFAULT true,
    deleted_at              TIMESTAMPTZ,
    deleted_by              UUID REFERENCES accounts(id),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),

    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL)
        OR
        (deleted_at IS NOT NULL AND deleted_by IS NOT NULL)
    )
);
```

규칙:

- browser login, MCP OAuth 2.1, device flow via authgate는 `kind='user'` account다.
- API key / agent key는 `kind='agent'` account다.
- 일반 product action에서 account hard delete는 하지 않는다.
- user 탈퇴나 agent 삭제는 `accounts.is_active=false`, `deleted_at`, `deleted_by`로 deactivate/soft delete한다.
- PII 원문은 평문으로 저장하지 않는다. 표시 이름과 사용자 상세 암호화 정책은 `docs/spec/security.md`를 따른다.
- `created_by`, `updated_by`, `deleted_by` 계열 컬럼은 모두 `accounts.id`를 참조한다.

## users

`users`는 authgate OAuth 로그인으로 확보된 사람 사용자 상세 테이블이다. `users.id`는
동시에 `accounts.id`다.

```sql
CREATE TABLE users (
    id                        UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    provider_sub_hash          TEXT UNIQUE,
    provider_sub_hash_version  INTEGER NOT NULL DEFAULT 1,
    email_ciphertext           BYTEA,
    email_nonce                BYTEA,
    email_hash                 TEXT,
    email_hash_version         INTEGER,
    anonymized_at              TIMESTAMPTZ
);
```

규칙:

- user 최초 생성과 탈퇴의 lifecycle side effect는 `docs/spec/lifecycle.md`를 따른다.
- user 탈퇴의 PII redaction/anonymization 상세 보안 정책은 `docs/spec/security.md`를 따른다.
- OAuth provider subject 원문은 저장하지 않고 provider/sub 기반 HMAC hash로 로그인 매칭한다.
- email 원문은 ciphertext로 저장하고, login/unique lookup이 필요하면 normalized email HMAC hash를 별도로 사용한다.
- 과거 `created_by`, `updated_by`, `deleted_by` 참조 보존을 위해 일반 product action으로 user row를 물리 삭제하지 않는다.

## account_encryption_keys

`account_encryption_keys`는 account별 PII data encryption key(DEK)를 key encryption key(KEK)로 wrap한 metadata를 저장한다. 구체적인 PII 분류, 암호화 방식,
rotation, crypto shredding 정책은 `docs/spec/security.md`를 따른다.

```sql
CREATE TABLE account_encryption_keys (
    account_id    UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    wrapped_dek   BYTEA,
    kek_id        TEXT NOT NULL,
    kek_version   TEXT,
    algorithm     TEXT NOT NULL DEFAULT 'AES-256-GCM',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    rewrapped_at  TIMESTAMPTZ,
    destroyed_at  TIMESTAMPTZ,

    CHECK (
        (destroyed_at IS NULL AND wrapped_dek IS NOT NULL)
        OR
        (destroyed_at IS NOT NULL AND wrapped_dek IS NULL)
    )
);
```

규칙:

- PII 원문 암호화와 HMAC lookup 정책은 `docs/spec/security.md`를 따른다.
- `destroyed_at`이 설정된 account encryption key는 PII 복호화에 사용할 수 없다.
- `wrapped_dek`, `kek_id`, `kek_version`은 plaintext PII가 아니지만 보안 민감 정보로 취급한다.

## agents

`agents`는 API key / CLI / MCP key로 접속할 수 있는 AI agent 상세 테이블이다.
`agents.id`는 동시에 `accounts.id`다.

```sql
CREATE TABLE agents (
    id         UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES accounts(id)
);
```

규칙:

- agent 삭제 상태는 `agents`가 아니라 공통 parent인 `accounts`에서 관리한다.
- agent 생성/삭제와 key 생성/revoke lifecycle은 `docs/spec/lifecycle.md`를 따른다.
- 한 user creator account가 만들 수 있는 active agent는 최대 `50`개다.
- 과거 `created_by`, `updated_by`, `deleted_by` 참조를 보존하기 위해 일반 product action으로 agent row를 물리 삭제하지 않는다.

## agent_keys

`agent_keys`는 agent 접속용 API key를 저장한다. token 원문은 저장하지 않고 hash만
저장한다.

```sql
CREATE TABLE agent_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id     UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    scopes       TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    created_by   UUID REFERENCES accounts(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    revoked_by   UUID REFERENCES accounts(id)
);
```

규칙:

- API key / agent key 인증은 항상 `agent` account로 처리한다.
- agent key는 agent 생성 시 자동 생성하지 않고 명시적인 key 생성 lifecycle로만 만든다. 자세한 정책은 `docs/spec/lifecycle.md`를 따른다.
- `revoked_at`이 있는 key는 인증에 사용할 수 없다.
- `expires_at <= now()`인 key는 인증에 사용할 수 없고 live key로 계산하지 않는다.
- `scopes`는 생략하거나 빈 배열이어야 한다. non-empty scopes는 받지 않는다.
- 한 agent가 동시에 가질 수 있는 live key는 최대 `10`개다.

Live key 조회/집계 보조 index:

```sql
CREATE INDEX agent_keys_agent_active_idx
    ON agent_keys(agent_id)
    WHERE revoked_at IS NULL;
```

## workspaces

workspace는 개인 노트 파일트리의 격리 경계다. workspace 권한은 `workspace_access` membership row가 source of truth이며, `workspaces.created_by`는 최초 생성자/audit attribution이다. 생성/삭제 side effect는 `docs/spec/lifecycle.md`를 따른다. Agent account는 공유받은 workspace에서 viewer/editor 작업자로만 동작하고 owner가 될 수 없다. 단일 workspace 제약 없이 자유롭게 생성/삭제할 수 있다.

```sql
CREATE TABLE workspaces (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    created_by  UUID NOT NULL REFERENCES accounts(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ,
    deleted_by  UUID REFERENCES accounts(id),
    purge_after TIMESTAMPTZ,

    CHECK (name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$'),
    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL)
        OR
        (deleted_at IS NOT NULL AND deleted_by IS NOT NULL AND purge_after IS NOT NULL)
    )
);

CREATE UNIQUE INDEX workspaces_created_by_name_live_uidx
    ON workspaces(created_by, name)
    WHERE deleted_at IS NULL;
```

규칙:

- REST 클라이언트는 `workspace_id`를 URL에 명시할 수 있다. `workspace_id`는 secret이 아니다.
- `workspaces.created_by`는 최초 생성자/audit attribution이다. 현재 권한 source는 `workspace_access`다.
- workspace 생성/삭제와 owner row 생성은 `docs/spec/lifecycle.md`를 따른다.
- workspace rename/delete/access 관리는 active `owner` role을 가진 user만 수행할 수 있다. agent는 grant를 받아도 lifecycle operation을 수행할 수 없다.
- 서버는 live workspace(`deleted_at IS NULL`)와 인증된 account의 effective role로 workspace 접근 권한을 검증한다.
- MCP/CLI path API는 요청 context에서 workspace를 resolve한 뒤 파일 path를 해석한다.
- workspace `name`은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이다. `/`, `:`, 공백은 허용하지 않는다.
- live workspace 이름은 `(created_by, name)` 기준으로 unique다. soft-deleted workspace 이름은 재사용할 수 있다.
- agent는 여러 user creator의 workspace를 공유받을 수 있으므로 workspace name은 global unique가 아니다.
- user creator account가 소유할 수 있는 live workspace는 최대 `20`개다.
- workspace가 생성되면 DB trigger가 canonical root node `/`를 같은 workspace에 만든다.
- 모든 workspace/list/file/search/access 조회는 live workspace만 대상으로 한다.

## workspace_access

`workspace_access`는 workspace membership과 workspace 단위 권한 테이블이다. notegate는 개인용 서비스이므로
초기에는 파일/폴더별 ACL을 두지 않는다.

```sql
CREATE TABLE workspace_access (
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    granted_by   UUID REFERENCES accounts(id),
    granted_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at   TIMESTAMPTZ,
    revoked_by   UUID REFERENCES accounts(id),

    PRIMARY KEY (workspace_id, account_id)
);
```

역할:

```text
owner  = read + write + workspace lifecycle 관리 + access 관리
editor = read + write
viewer = read

read  = ls/stat/read/find/grep
write = mkdir/touch/write/patch/mv/rm
```

`workspace_access.role='owner'`가 workspace owner의 source of truth다. `workspaces.created_by`는
최초 생성자/audit attribution이며 현재 권한 판정에 직접 쓰지 않는다.

성능 규칙:

- 요청 시작 시 live workspace(`workspaces.deleted_at IS NULL`)와 effective role을 확인한다.
- caller의 effective role은 `workspace_access`의 live owner/editor/viewer row에서 계산한다.
- 파일 목록/검색/읽기 쿼리는 권한 확인 이후 `workspace_id` 조건으로 실행한다.
- `revoked_at`이 있는 access row는 권한으로 인정하지 않는다.
- `accounts.is_active=false`이거나 `accounts.deleted_at IS NOT NULL`인 account는 live access로 인정하지 않는다.
- `owner` role은 active user account에만 부여할 수 있다. Agent account는 `viewer` 또는 `editor`만 받을 수 있다.
- owner row 생성, 마지막 owner 보호, creator owner row 보호는 `docs/spec/lifecycle.md`를 따른다.
- 한 workspace의 active access row는 최대 `20`개다. 생성 시 자동 owner row도 이 제한에 포함한다.
- `granted_by`/`granted_at`은 현재 live grant 상태를 마지막으로 부여하거나 재활성화한 actor와 시각이다.

Caller의 live workspace 조회 보조 index:

```sql
CREATE INDEX workspace_access_account_idx
    ON workspace_access(account_id)
    WHERE revoked_at IS NULL;
```

Owner 존재 확인과 owner revoke/downgrade 보호를 위한 보조 index:

```sql
CREATE INDEX workspace_access_owner_active_idx
    ON workspace_access(workspace_id, account_id)
    WHERE revoked_at IS NULL AND role = 'owner';
```

Owner-row invariant는 단일 row CHECK만으로 표현할 수 없으므로 `docs/spec/lifecycle.md`의 owner 보호 규칙에 따라 service transaction에서 검증한다.

## nodes

`nodes`는 folder와 document의 공통 tree entry다. directory 위치의 source of truth는
`parent_id + name`이다. 전체 path는 저장된 canonical 값이 아니라 parent chain에서 derive한다.

```sql
CREATE TABLE nodes (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id     UUID,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('folder', 'document')),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_by    UUID NOT NULL REFERENCES accounts(id),
    updated_by    UUID NOT NULL REFERENCES accounts(id),
    deleted_by    UUID REFERENCES accounts(id),
    purge_after   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,

    UNIQUE (id, workspace_id),
    FOREIGN KEY (parent_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,

    CHECK (
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND deleted_at IS NULL)
        OR
        (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (parent_id IS NULL OR name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$'),
    CHECK (name NOT IN ('.', '..')),
    CHECK (kind <> 'document' OR name LIKE '%.md'),
    CHECK (kind <> 'folder' OR parent_id IS NULL OR name NOT LIKE '%.md'),
    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL)
        OR
        (deleted_at IS NOT NULL AND deleted_by IS NOT NULL AND purge_after IS NOT NULL)
    )
);
```

`nodes.created_by`는 node를 만든 account, `updated_by`는 마지막으로 node metadata를
바꾼 account, `deleted_by`는 delete를 요청한 account다. `purge_after`는 내부 purge job이
hard delete할 수 있는 가장 이른 시각이다. document content 변경자는 `documents.updated_by`에 기록한다.

`sort_order`는 같은 parent folder 안에서 사용자 지정 정렬을 위한 optional ordering key다.
기본값 `0`이면 이름순 fallback을 사용한다.

### Root invariant

각 workspace는 root folder node 하나를 가진다.

```sql
CREATE UNIQUE INDEX nodes_one_root_per_workspace
    ON nodes(workspace_id)
    WHERE parent_id IS NULL;
```

`parent_id IS NULL`은 root에만 허용한다. root는 soft delete 대상이 아니며,
root 이동/삭제/rename은 conflict다. `workspaces.root_node_id`는 두지 않고
`nodes(parent_id IS NULL)`로 root를 찾는다.

삭제 예정 node를 찾기 위한 purge index:

```sql
CREATE INDEX nodes_purge_due_idx
    ON nodes(purge_after, workspace_id, id)
    WHERE deleted_at IS NOT NULL;
```

Workspace 생성 시 root 자동 생성:

```sql
CREATE TRIGGER workspaces_create_root_node
AFTER INSERT ON workspaces
FOR EACH ROW
EXECUTE FUNCTION create_workspace_root_node();
```

### Path derivation and lookup

Canonical location은 `(workspace_id, parent_id, name)`이다. full-path cache를 canonical로
저장하지 않는다. 따라서 folder move/rename 시 descendant row의 path를 대량 update하지
않는다. descendants의 path는 parent chain 변화로 논리적으로 바뀐다.

규칙:

- root path는 `/`다.
- root 아래 path는 ancestor name을 `/`로 join해서 만든다.
- path resolve는 path segment를 나눈 뒤 root부터 `(workspace_id, parent_id, name)` lookup을 반복한다.
- 최대 depth가 `5`이므로 path resolve는 최대 5번의 indexed lookup으로 제한된다.
- 응답의 `path` 필드는 항상 derive한 display 값이다.
- path uniqueness는 별도 full-path unique index가 아니라 sibling unique invariant와 tree invariant로 보장한다.

### Depth, fanout, and workspace size limits

```text
workspace_max_nodes = 10000
workspace_max_documents = 5000
workspace_max_document_bytes = 268435456 bytes
max_path_depth = 5
max_path_len = 645 bytes
folder_max_children = 200
```

규칙:

- depth는 저장하지 않고 parent chain에서 계산한다. root는 `0`, root의 직접 자식은 `1`이다.
- create/touch/write(create=true)는 resulting depth가 `5` 또는 resulting path 길이 `645` bytes를 넘으면 거부한다.
- move는 이동되는 subtree 전체의 resulting max depth가 `5` 이하인지 transaction 안에서 검증한다.
- move/rename은 moved node의 `parent_id`/`name`만 바꾸며 descendant path rewrite를 하지 않는다.
- 같은 parent folder의 live direct children은 최대 `200`개다.
- workspace 안 live nodes는 최대 `10000`개다.
- workspace 안 live documents는 최대 `5000`개다. document create transaction에서 검사한다.
- workspace 안 live document 원문 총량은 최대 `268435456` bytes다. write/patch transaction에서 검사한다.
- child 수와 workspace node 수 제한은 partial unique/check constraint로 표현하기 어렵기 때문에 create/move transaction에서 count 후 검증한다.
- `max_path_len=645`는 ASCII node name 최대 `128` chars와 depth `5`에서 도출되는 최대 absolute path 길이다.

### Name constraints

Root를 제외한 node name은 CLI/MCP path가 안전하게 파싱되도록 제한한다.

```text
workspace name max length      = 63 chars
folder name max length         = 128 chars
document file name max length  = 128 chars, including .md
document title stem max length = 125 chars, excluding .md
node name regex                = ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$
```

규칙:

- `/`, `:`, 공백, control character는 허용하지 않는다.
- `.`와 `..`는 허용하지 않는다.
- document name은 추가로 `.md`로 끝나야 한다.
- document 제목은 현재 별도 `title` 컬럼이 아니라 filename stem으로 본다. 예: `meeting-note.md`의 제목 stem은 `meeting-note`다.
- document filename 전체 길이는 `.md` 포함 최대 `128` chars, stem은 최대 `125` chars다.
- folder name은 최대 `128` chars이고 `.md`로 끝날 수 없다.
- Unicode 파일명은 초기 설계에서 제외한다. 필요하면 normalization/collation 정책을 별도 결정한다.

### Sibling name uniqueness

일반 파일시스템처럼 같은 folder 안에서만 이름 중복을 금지한다. 다른 folder에서는
같은 이름을 사용할 수 있다.

```sql
CREATE UNIQUE INDEX nodes_live_sibling_name_key
    ON nodes(workspace_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
```

의미:

```text
/projects/readme.md   허용
/archive/readme.md    허용
/projects/readme.md   같은 parent 안 중복이면 거부
```

`kind`는 unique key에 넣지 않는다. 같은 folder 안 namespace는 folder와 document가
공유한다.

### Listing indexes

폴더 직접 자식 조회는 keyset pagination을 전제로 한다.

```sql
CREATE INDEX nodes_children_idx
    ON nodes(workspace_id, parent_id, sort_order, name, id)
    WHERE deleted_at IS NULL;
```

검색 보조 index:

```sql
CREATE INDEX nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;
```

## documents

`documents`는 document node의 Markdown 원문을 저장한다.

```sql
CREATE TABLE documents (
    node_id        UUID PRIMARY KEY,
    workspace_id   UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md     TEXT NOT NULL DEFAULT '',
    content_sha256 TEXT NOT NULL DEFAULT '',
    byte_len       INTEGER NOT NULL DEFAULT 0,
    line_count     INTEGER NOT NULL DEFAULT 0,
    created_by     UUID NOT NULL REFERENCES accounts(id),
    updated_by     UUID NOT NULL REFERENCES accounts(id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (node_id, workspace_id),
    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0 AND byte_len <= 524288),
    CHECK (line_count >= 0 AND line_count <= 2000)
);
```

규칙:

- document node만 document row를 가진다.
- folder node는 document row를 가지지 않는다.
- 저장 시 `documents.updated_at`, `documents.updated_by`, 연결된 `nodes.updated_at`, `nodes.updated_by`를 함께 갱신한다.
- `byte_len`과 `line_count`는 read limit, pagination, write guard에 사용한다.
- document create는 `workspace_max_nodes=10000`과 `workspace_max_documents=5000`을 모두 만족해야 한다.
- 문서는 개별 최대 `524288` bytes, `2000` lines까지만 저장한다.
- workspace의 live document 원문 총량은 최대 `268435456` bytes다. 초과 시 문서를 나누거나 workspace를 분리하도록 유도한다.

검색/정렬 보조 index:

```sql
CREATE INDEX documents_content_trgm_idx
    ON documents USING gin (content_md gin_trgm_ops);

CREATE INDEX documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC, node_id);
```

## Soft delete and purge

- account/user/workspace/agent/node 삭제 lifecycle은 `docs/spec/lifecycle.md`를 따른다.
- node delete는 사용자-facing 복구 기능을 제공하지 않는다. soft delete는 비동기 hard delete를 위한 내부 상태다.
- query는 반드시 `accounts.is_active`, `revoked_at`, `nodes.deleted_at IS NULL`을 고려한다.
- `workspaces.purge_after <= now()`인 deleted workspace는 내부 purge job으로 hard delete될 수 있다. 이때 `workspace_access`, `nodes`, `documents`는 FK cascade로 제거된다.
- `nodes.purge_after <= now()`인 deleted node/document는 내부 purge job으로 hard delete될 수 있다.
- purge job은 모든 서버 인스턴스에서 시작될 수 있지만, Postgres advisory transaction lock을 사용해 같은 DB에서 한 번에 하나의 purge transaction만 실행한다. Lock을 얻지 못한 worker tick은 즉시 skip한다.
- purge job은 bounded batch로 실행한다. 현재 batch는 workspace 최대 100개, node 최대 1000개다.
- 기본 node/workspace retention은 30일이다.

## Reset policy

현재 단계에서 프로덕션 데이터가 없다면 migration을 누적 보정하지 않고, 새 스키마로
squash/reset하는 것을 허용한다. 프로덕션 데이터가 생기면 이후부터는 forward-only
migration만 허용한다.
