# 파일시스템 Vault DB 스키마

개인용 Markdown vault를 구현하기 위한 DB 스키마 설계다.

목표는 특정 root 아래에서 폴더와 `.md` 파일만 다루는 작은 파일시스템이다.
API는 `ls`, `mkdir`, `touch`, `open`, `save`, `mv`, `rm`, `find`, `grep`을
제공한다.

이 스키마는 파일트리 vault core만 책임진다. AI 검색, AI 문서 수정,
daily/context/review, raw data 생명주기 관리는 나중에 이 구조 위에 얹는다.

## 기존 전제

`users` 테이블은 이미 존재한다.

현재 DB에는 최소한 다음 user 필드가 있다.

```text
id
email
sub
display_name
is_active
created_at
updated_at
```

vault 스키마는 새 `users`를 만들지 않고 기존 `users.id`를 참조한다.

## 테이블

MVP 테이블은 3개다.

```text
workspaces
nodes
documents
```

관계는 다음과 같다.

```text
users
`-- workspaces
    `-- nodes
        |-- folder node
        `-- document node
            `-- documents
```

## workspaces

workspace는 사용자의 개인 vault 경계다.

처음에는 사용자당 `default` workspace 하나만 만들어도 된다. 그래도
workspace를 두는 이유는 개인 데이터의 경계를 명확히 하기 위해서다.

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

- `owner_user_id`는 vault 소유자다.
- 클라이언트는 `workspace_id`를 보내지 않는다.
- API는 인증된 사용자로부터 workspace를 결정한다.

### Workspace 초기화

사용자가 Google Login 후 처음 `notegate`에 접근하면 서버는 다음을 보장한다.

1. `authgate`가 검증한 사용자 정보로 기존 `users.id`를 확인한다.
2. 해당 사용자에게 `default` workspace가 없으면 생성한다.
3. 해당 workspace에 root node가 없으면 생성한다.

초기 root node:

```text
name       = /
kind       = folder
parent_id  = null
path_cache = /
```

이 초기화는 idempotent해야 한다.

## nodes

`nodes`는 파일 트리 자체다.

폴더와 문서를 같은 트리에서 다뤄야 하므로 둘 다 `nodes`에 들어간다.

```text
kind = folder
kind = document
```

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
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND path_cache = '/')
        OR
        (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (path_cache LIKE '/%'),
    CHECK (kind <> 'document' OR name LIKE '%.md'),
    CHECK (kind <> 'folder' OR parent_id IS NULL OR name NOT LIKE '%.md')
);
```

### Root node

각 workspace는 명시적인 root node 하나를 가진다.

```text
name       = /
kind       = folder
parent_id  = null
path_cache = /
```

root node는 삭제하거나 이동하거나 이름을 바꾸지 않는다.

root 중복을 막기 위해 partial unique index를 둔다.

```sql
CREATE UNIQUE INDEX nodes_one_root_per_workspace
    ON nodes(workspace_id)
    WHERE parent_id IS NULL;
```

이 index에는 `deleted_at IS NULL`을 넣지 않는다. root는 삭제할 수 없다는
전제를 더 엄격하게 지키기 위해서다. root가 실수로 soft delete되어도 같은
workspace에 새 root를 만들 수 없어야 한다.

### 같은 폴더 안 이름 중복 방지

soft delete를 쓰기 때문에 일반 `UNIQUE (workspace_id, parent_id, name)`은
맞지 않다. 삭제된 파일 이름은 나중에 다시 쓸 수 있어야 한다.

살아있는 node끼리만 같은 parent 아래에서 이름 중복을 막는다.

```sql
CREATE UNIQUE INDEX nodes_live_sibling_name_key
    ON nodes(workspace_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
```

### Path 중복 방지

`resolve`는 `path_cache`로 자주 조회한다. 살아있는 node 기준으로 같은
workspace 안에서 path가 중복되면 안 된다.

```sql
CREATE UNIQUE INDEX nodes_live_path_key
    ON nodes(workspace_id, path_cache)
    WHERE deleted_at IS NULL;
```

이 index의 역할:

- `resolve?path=...` 조회를 빠르게 한다.
- move/rename 버그로 `path_cache` 중복이 생기는 것을 막는다.
- soft delete된 path는 나중에 재사용할 수 있게 한다.

### 조회용 index

`ls`, `find`, `mv`, `rm`에 필요한 index다.

```sql
CREATE INDEX nodes_children_idx
    ON nodes(workspace_id, parent_id, sort_order, name)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;
```

### path_cache 생성 규칙

`path_cache`는 클라이언트 입력을 그대로 저장하지 않는다.

항상 서버가 다음 방식으로 계산한다.

```text
root path = /
child path = join(parent.path_cache, normalized_name)
```

규칙:

- root path는 `/`다.
- root 외 node 이름에는 `/`가 들어갈 수 없다.
- folder path에는 trailing slash를 저장하지 않는다.
- 중복 slash, `.`, `..` segment는 canonical path에 들어갈 수 없다.
- move/rename 후에는 영향을 받는 descendant path도 함께 갱신한다.

### 이름 규칙

root를 제외한 node name은 다음 규칙을 따른다.

- 빈 문자열일 수 없다.
- `/`를 포함할 수 없다.
- `.` 또는 `..`일 수 없다.
- document node는 소문자 `.md`로 끝나야 한다.
- folder node는 `.md`로 끝날 수 없다.

DB CHECK는 최소 방어선이다. 실제 검증과 사용자에게 반환할 에러 메시지는
application layer에서 처리한다.

### DB가 직접 강제하지 않는 규칙

PostgreSQL `CHECK`는 다른 row를 조회할 수 없으므로 아래 규칙은 DB CHECK만
으로는 강제하지 않는다. MVP에서는 application layer에서 검증한다.

- `parent_id`가 있으면 parent는 같은 workspace의 folder node여야 한다.
- document row는 `kind = document`인 node에만 붙어야 한다.
- folder node에는 document row를 만들 수 없다.

나중에 필요하면 trigger로 보강할 수 있다.

## documents

`documents`는 Markdown 본문을 저장한다.

`kind = document`인 node만 `documents` row를 가진다. folder node는
document row를 가지지 않는다.

```sql
CREATE TABLE documents (
    node_id       UUID PRIMARY KEY,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md    TEXT NOT NULL DEFAULT '',
    search_text   TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE
);
```

`workspace_id`는 `nodes`에도 있으므로 중복이다. 그래도 `grep`에서
workspace 단위로 빠르게 제한하기 위해 둔다.

`documents.workspace_id`와 연결된 `nodes.workspace_id`가 항상 같도록
composite foreign key로 강제한다.

### 검색용 index

MVP에서는 전문 검색 엔진을 쓰지 않는다.

`grep`은 `search_text ILIKE`로 후보 문서를 찾고, 애플리케이션 코드에서
`content_md`를 줄 단위로 다시 검사한다.

```sql
CREATE INDEX documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC);
```

나중에 PostgreSQL trigram을 쓰기로 결정하면 그때 `search_text`용 GIN index를
추가한다. MVP에서는 필수로 보지 않는다.

## updated_at 갱신 정책

MVP에서는 `updated_at`을 application layer에서 갱신한다.

- 문서 저장 시 `documents.updated_at`과 연결된 `nodes.updated_at`을 함께 갱신한다.
- node 이름 변경, 이동, 삭제 시 `nodes.updated_at`을 갱신한다.
- folder 이동 시 descendant의 `path_cache`가 바뀌면 descendant의 `updated_at`도 갱신할 수 있다.

나중에 필요하면 DB trigger로 보강한다.

## 명령별 DB 동작

### ls

폴더의 직접 자식만 조회한다.

```sql
SELECT id, parent_id, name, kind, path_cache, sort_order
FROM nodes
WHERE workspace_id = $1
  AND parent_id = $2
  AND deleted_at IS NULL
ORDER BY sort_order, name;
```

### mkdir

folder node 하나를 만든다.

```text
INSERT nodes(kind = 'folder')
```

규칙:

- parent는 같은 workspace의 folder node여야 한다.
- 같은 parent 아래에 같은 이름의 살아있는 node가 있으면 실패한다.

### touch

`.md` document를 만든다.

하나의 transaction에서 처리한다.

```text
BEGIN
  INSERT nodes(kind = 'document')
  INSERT documents(node_id = new node id)
COMMIT
```

규칙:

- parent는 같은 workspace의 folder node여야 한다.
- name은 `.md`로 끝나야 한다.
- 초기 `content_md`와 `search_text`는 빈 문자열이다.

### open

document node와 Markdown 본문을 조회한다.

```sql
SELECT n.id, n.name, n.path_cache, d.content_md
FROM nodes n
JOIN documents d
  ON d.node_id = n.id
 AND d.workspace_id = n.workspace_id
WHERE n.workspace_id = $1
  AND n.id = $2
  AND n.kind = 'document'
  AND n.deleted_at IS NULL;
```

### save

Markdown 본문과 검색용 텍스트를 갱신한다.

```text
UPDATE documents.content_md
UPDATE documents.search_text
UPDATE documents.updated_at
UPDATE nodes.updated_at
```

`search_text`는 원본이 아니라 파생 데이터다. 기본값은 정규화된
`content_md`로 충분하다.

MVP에서는 다음처럼 단순하게 시작한다.

```text
search_text = content_md
```

나중에 필요하면 `normalize_for_search(content_md)`를 도입한다.

가능한 정규화 규칙:

- line ending을 `\n`으로 통일한다.
- 대소문자 처리는 DB `ILIKE`에 맡긴다.
- Markdown parser로 본문을 추출하는 작업은 MVP에서 하지 않는다.

### mv

이동과 이름 변경을 같은 operation으로 처리한다.

```text
new_parent_id 변경
name 변경 가능
path_cache 갱신
folder면 descendant path_cache도 갱신
```

반드시 transaction으로 처리한다.

규칙:

- root node는 이동하거나 이름을 바꿀 수 없다.
- target parent는 같은 workspace의 folder node여야 한다.
- 자기 자신이나 자신의 descendant 아래로 이동할 수 없다.
- document node의 새 이름은 소문자 `.md`로 끝나야 한다.
- folder node의 새 이름은 `.md`로 끝날 수 없다.
- 같은 target parent 아래에 같은 이름의 살아있는 node가 있으면 실패한다.

folder를 이동하거나 이름 변경하면 descendant path도 갱신한다. 기본 구현은
기존 path prefix를 새 path prefix로 바꾸는 방식으로 충분하다.

```sql
UPDATE nodes
SET path_cache = $new_prefix || substring(path_cache from length($old_prefix) + 1),
    updated_at = now()
WHERE workspace_id = $workspace_id
  AND deleted_at IS NULL
  AND (
    id = $moving_node_id
    OR path_cache LIKE $old_prefix || '/%'
  );
```

이 처리는 반드시 같은 transaction 안에서 실행한다.

### rm

soft delete한다.

```text
nodes.deleted_at = now()
```

folder 삭제는 모든 descendant도 함께 soft delete한다.

`documents` row는 지우지 않는다. 연결된 node가 soft deleted 상태이므로
`ls`, `find`, `grep`에서 보이지 않는다.

root node는 삭제할 수 없다.

folder descendant 삭제는 recursive CTE로 처리한다.

```sql
WITH RECURSIVE descendants AS (
    SELECT id
    FROM nodes
    WHERE workspace_id = $1
      AND id = $2

    UNION ALL

    SELECT n.id
    FROM nodes n
    JOIN descendants d
      ON n.parent_id = d.id
    WHERE n.workspace_id = $1
      AND n.deleted_at IS NULL
)
UPDATE nodes
SET deleted_at = now(),
    updated_at = now()
WHERE workspace_id = $1
  AND id IN (SELECT id FROM descendants);
```

이 처리도 transaction 안에서 실행한다.

## find

`find`는 `nodes`만 본다.

```sql
SELECT id, parent_id, name, kind, path_cache
FROM nodes
WHERE workspace_id = $1
  AND deleted_at IS NULL
  AND path_cache ILIKE '%' || $2 || '%'
ORDER BY path_cache
LIMIT 50;
```

선택 필터:

```sql
AND kind = 'document'
AND path_cache LIKE $3 || '%'
```

## grep

`grep`은 먼저 `documents.search_text`로 후보를 찾는다.

```sql
SELECT n.id AS node_id, n.path_cache, d.content_md
FROM documents d
JOIN nodes n
  ON n.id = d.node_id
 AND n.workspace_id = d.workspace_id
WHERE d.workspace_id = $1
  AND n.deleted_at IS NULL
  AND d.search_text ILIKE '%' || $2 || '%'
ORDER BY d.updated_at DESC
LIMIT 50;
```

path 제한이 있으면:

```sql
AND n.path_cache LIKE $3 || '%'
```

그 다음 애플리케이션 코드에서 `content_md`를 줄 단위로 나눠 실제 matching
line, line number, context를 만든다.

## 불변식

- 클라이언트는 `workspace_id`를 보내지 않는다.
- 모든 query와 mutation은 인증된 사용자의 workspace 안에서만 실행한다.
- workspace마다 root node는 정확히 하나다.
- root node는 삭제, 이동, 이름 변경할 수 없다.
- node는 정확히 하나의 workspace에 속한다.
- parent node는 child node와 같은 workspace에 속해야 한다.
- parent node는 반드시 folder node여야 한다.
- folder와 document는 모두 `nodes`에 저장한다.
- document node만 `documents` row를 가진다.
- folder node는 `documents` row를 가지지 않는다.
- document node 이름은 소문자 `.md`로 끝나야 한다.
- folder node 이름은 `.md`로 끝날 수 없다.
- `node_id`는 정체성이다.
- `path_cache`는 표시와 검색 범위 제한을 위한 cache다.
- `path_cache`는 서버가 계산하며 클라이언트 입력을 그대로 저장하지 않는다.
- move/rename은 영향을 받는 descendant path도 갱신한다.
- 삭제된 node는 `ls`, `resolve`, `find`, `grep`에서 보이지 않는다.

## 나중에 추가할 수 있는 것

지금은 넣지 않는다.

```text
document_lines      grep을 더 빠르게 하고 싶을 때
document_versions   문서 변경 이력이 필요할 때
trash/restore       삭제 복원이 필요할 때
trigram index       ILIKE 검색이 느려질 때
```
