# Files DB schema

notegate는 개인 Markdown 파일시스템을 Postgres에 저장한다. DB는 source of
truth이며, REST와 MCP는 모두 이 구조 위에서 동작한다.

## 테이블 개요

Canonical tables:

```text
accounts          user/agent 공통 actor identity
users             authgate OAuth 사용자 상세
agents            AI agent 상세
agent_keys        agent API key credential hash
workspaces        개인 노트 workspace 경계
workspace_access  workspace 단위 viewer/editor/owner 권한
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
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind         TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name TEXT NOT NULL DEFAULT '',
    is_active    BOOLEAN NOT NULL DEFAULT true,
    deleted_at   TIMESTAMPTZ,
    deleted_by   UUID REFERENCES accounts(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

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
- `created_by`, `updated_by`, `deleted_by` 계열 컬럼은 모두 `accounts.id`를 참조한다.

## users

`users`는 authgate OAuth 로그인으로 확보된 사람 사용자 상세 테이블이다. `users.id`는
동시에 `accounts.id`다.

```sql
CREATE TABLE users (
    id            UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    sub           TEXT UNIQUE,
    email         TEXT,
    anonymized_at TIMESTAMPTZ
);
```

규칙:

- user 탈퇴는 account deactivate와 user PII redaction/anonymization으로 처리한다.
- 과거 `created_by`, `updated_by`, `deleted_by` 참조 보존을 위해 일반 product action으로 user row를 물리 삭제하지 않는다.

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
- agent 삭제는 account deactivate/soft delete다.
- agent 삭제 시 active `agent_keys`와 `workspace_access`를 함께 revoke한다.
- 한 creator account가 만들 수 있는 active agent는 최대 `50`개다.
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
- `revoked_at`이 있는 key는 인증에 사용할 수 없다.
- scopes는 workspace role 권한을 넓히지 않고 줄이는 용도로만 사용한다.
- 한 agent가 동시에 가질 수 있는 active key는 최대 `10`개다.

## workspaces

workspace는 개인 노트 파일트리의 격리 경계다. workspace는 user account도, agent
account도 소유할 수 있으며, 단일/default workspace 제약 없이 자유롭게 생성/삭제할 수 있다.

```sql
CREATE TABLE workspaces (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_account_id UUID NOT NULL REFERENCES accounts(id),
    name             TEXT NOT NULL,
    created_by       UUID NOT NULL REFERENCES accounts(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (owner_account_id, name),
    CHECK (name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$')
);
```

규칙:

- REST 클라이언트는 `workspace_id`를 URL에 명시할 수 있다. `workspace_id`는 secret이 아니다.
- 서버는 인증된 account와 `workspace_access`로 workspace 접근 권한을 검증한다.
- MCP/CLI path API는 요청 context에서 workspace를 resolve한 뒤 파일 path를 해석한다.
- workspace `name`은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이다. `/`, `:`, 공백은 허용하지 않는다.
- `(owner_account_id, name)`은 unique다. 사용자/agent 자신의 workspace는 이름만으로 안정적으로 선택할 수 있다.
- 다른 owner의 workspace를 agent가 함께 볼 수 있으므로 name은 global unique가 아니다.
- owner account가 소유할 수 있는 workspace는 최대 `20`개다. workspace 생성 transaction에서 검사한다.
- workspace 생성자는 `workspace_access.role = 'owner'`를 자동으로 받는다.
- workspace가 생성되면 DB trigger가 canonical root node `/`를 같은 workspace에 만든다.
- workspace 삭제는 일반 owner operation이며 해당 workspace의 access row, node, document 전체 삭제를 의미한다.

## workspace_access

`workspace_access`는 workspace 단위 권한 테이블이다. notegate는 개인용 서비스이므로
초기에는 파일/폴더별 ACL을 두지 않는다.

```sql
CREATE TABLE workspace_access (
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('viewer', 'editor', 'owner')),
    created_by   UUID REFERENCES accounts(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at   TIMESTAMPTZ,
    revoked_by   UUID REFERENCES accounts(id),

    PRIMARY KEY (workspace_id, account_id)
);
```

역할:

```text
viewer = list/stat/read/find/grep
editor = viewer + write/patch/mkdir/touch/move/delete
owner  = editor + workspace access and agent key management
```

성능 규칙:

- 요청 시작 시 `workspace_access`로 workspace 권한을 확인한다.
- 파일 목록/검색/읽기 쿼리는 권한 join 없이 `workspace_id` 조건으로 실행한다.
- `revoked_at`이 있는 access row는 권한으로 인정하지 않는다.
- 한 workspace의 active access account는 최대 `20`개다. grant transaction에서 검사한다.

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
        (deleted_at IS NULL AND deleted_by IS NULL)
        OR
        (deleted_at IS NOT NULL AND deleted_by IS NOT NULL)
    )
);
```

`nodes.created_by`는 node를 만든 account, `updated_by`는 마지막으로 node metadata를
바꾼 account, `deleted_by`는 soft delete한 account다. document content 변경자는
`documents.updated_by`에 기록한다.

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
max_path_len = 768 bytes
folder_max_children = 200
```

규칙:

- depth는 저장하지 않고 parent chain에서 계산한다. root는 `0`, root의 직접 자식은 `1`이다.
- create/touch/write(create=true)는 resulting depth가 `5` 또는 resulting path 길이 `768` bytes를 넘으면 거부한다.
- move는 이동되는 subtree 전체의 resulting max depth가 `5` 이하인지 transaction 안에서 검증한다.
- move/rename은 moved node의 `parent_id`/`name`만 바꾸며 descendant path rewrite를 하지 않는다.
- 같은 parent folder의 live direct children은 최대 `200`개다.
- workspace 안 live nodes는 최대 `10000`개다.
- workspace 안 live documents는 최대 `5000`개다. document create transaction에서 검사한다.
- workspace 안 live document 원문 총량은 최대 `268435456` bytes다. write/patch transaction에서 검사한다.
- child 수와 workspace node 수 제한은 partial unique/check constraint로 표현하기 어렵기 때문에 create/move/restore transaction에서 count 후 검증한다.

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

## Soft delete

- account 삭제는 `accounts.deleted_at`, `accounts.deleted_by`, `accounts.is_active=false`로 처리한다.
- node 삭제는 `nodes.deleted_at`과 `nodes.deleted_by`를 설정한다.
- query는 반드시 `accounts.is_active`, `revoked_at`, `nodes.deleted_at IS NULL`을 고려한다.

장기적으로는 retention 정책에 따라 soft-deleted document를 purge하는 job을 둘 수 있다.

## Reset policy

현재 단계에서 프로덕션 데이터가 없다면 migration을 누적 보정하지 않고, 새 스키마로
squash/reset하는 것을 허용한다. 프로덕션 데이터가 생기면 이후부터는 forward-only
migration만 허용한다.
