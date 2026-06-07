# Files DB schema

notegate는 개인 Markdown 파일시스템을 Postgres에 저장한다. DB는 source of
truth이며, REST와 MCP는 모두 이 구조 위에서 동작한다.

## 테이블 개요

Canonical tables:

```text
users        기존 인증 사용자 테이블
workspaces   사용자별 파일트리 경계
nodes        folder/document 공통 tree node
documents    markdown document 본문
```

현재 단계는 원본 Markdown 저장만 고려한다. grep은 `documents.content_md`를 `ILIKE`로
후보 검색하고 application code에서 line-split한다.

## users

`users`는 authgate OAuth 로그인으로 확보된 기존 사용자 테이블이다. Files 스키마는
새 user를 만들지 않고 `users.id`를 참조한다.

필요 필드:

```text
id
email
sub
display_name
is_active
created_at
updated_at
```

## workspaces

workspace는 사용자 파일트리의 격리 경계다. 초기 제품은 사용자당 `default`
workspace 하나를 사용한다.

```sql
CREATE TABLE workspaces (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (owner_user_id, name)
);
```

규칙:

- 클라이언트는 `workspace_id`를 보내지 않는다.
- 서버는 인증된 `users.id`로 default workspace를 찾거나 초기화한다.
- workspace가 생성되면 DB trigger가 canonical root node `/`를 같은 workspace에 만든다.
- workspace 삭제는 해당 사용자의 파일트리 전체 삭제를 의미한다.

## nodes

`nodes`는 folder와 document의 공통 tree entry다. directory 경로는 별도 table이
아니라 `parent_id` tree와 `path_cache`로 표현한다.

```sql
CREATE TABLE nodes (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id     UUID,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('folder', 'document')),
    path_cache    TEXT NOT NULL,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,

    UNIQUE (id, workspace_id),
    FOREIGN KEY (parent_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,

    CHECK (
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND path_cache = '/' AND deleted_at IS NULL)
        OR
        (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (path_cache LIKE '/%'),
    CHECK (kind <> 'document' OR name LIKE '%.md'),
    CHECK (kind <> 'folder' OR parent_id IS NULL OR name NOT LIKE '%.md')
);
```

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

### Path uniqueness

살아있는 node의 canonical path는 workspace 안에서 unique해야 한다.

```sql
CREATE UNIQUE INDEX nodes_live_path_key
    ON nodes(workspace_id, path_cache)
    WHERE deleted_at IS NULL;
```

이 index는 path lookup을 빠르게 하고, move/rename 버그로 path가 충돌하는 것을
막는 최종 방어선이다.

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

CREATE INDEX nodes_path_trgm_idx
    ON nodes USING gin (path_cache gin_trgm_ops)
    WHERE deleted_at IS NULL;
```

## documents

`documents`는 document node의 Markdown 원문을 저장한다.

```sql
CREATE TABLE documents (
    node_id       UUID PRIMARY KEY,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md    TEXT NOT NULL DEFAULT '',
    content_sha256 TEXT NOT NULL DEFAULT '',
    byte_len      INTEGER NOT NULL DEFAULT 0,
    line_count    INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0),
    CHECK (line_count >= 0)
);
```

규칙:

- document node만 document row를 가진다.
- folder node는 document row를 가지지 않는다.
- 저장 시 `documents.updated_at`과 연결된 `nodes.updated_at`을 함께 갱신한다.
- `byte_len`과 `line_count`는 read limit, pagination, write guard에 사용한다.

## Soft delete

삭제는 `nodes.deleted_at`을 설정한다. query는 반드시 `nodes.deleted_at IS NULL`을 확인한다.

장기적으로는 retention 정책에 따라 soft-deleted document를 purge하는 job을 둘 수 있다.

## Reset policy

현재 단계에서 프로덕션 데이터가 없다면 migration을 누적 보정하지 않고, 새 스키마로
squash/reset하는 것을 허용한다. 프로덕션 데이터가 생기면 이후부터는 forward-only
migration만 허용한다.
