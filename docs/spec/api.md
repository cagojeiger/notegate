# 파일시스템 Vault API

개인용 Markdown vault의 1차 API 설계다.

목표는 브라우저에서 특정 root 아래를 파일시스템처럼 다루는 것이다. 이
root 안에서는 폴더와 `.md` 파일만 만들 수 있다.

```text
/
|-- projects/
|   `-- memgrep.md
`-- archive/
```

UI는 전체 트리를 한 번에 가져오지 않는다. `ls`처럼 사용자가 폴더를 열 때
그 폴더의 직접 자식만 가져온다.

## 제품 경계

이 프로젝트의 1차 목표는 스마트 메모앱이 아니라 파일트리 메모장이다.

지금 API가 책임지는 범위:

```text
ls / mkdir / touch / open / save / mv / rm / find / grep
```

나중에 이 API 위에 얹을 범위:

```text
AI 검색
AI 문서 수정
daily/context/review
archive/lifecycle
raw data 정리
```

1차 API는 폴더와 Markdown 문서를 파일시스템처럼 다루는 최소 명령만
제공한다.

## 인증 경계

이 프로젝트 이름은 `notegate`다.

`notegate`는 자체 인증 시스템을 구현하지 않는다. 모든 인증은 `authgate`를
통해 처리한다.

1차 인증 방식은 Google Login만 지원한다.

`notegate` API는 `authgate`가 검증한 사용자 정보만 신뢰한다.

클라이언트는 `user_id`나 `workspace_id`를 직접 보내지 않는다. 서버는
인증된 사용자로부터 기존 `users.id`를 확인하고, 해당 사용자의 `default`
workspace를 결정한다.

## 목표 명령

1차 목표 명령은 이 정도로 제한한다.

```text
ls      폴더 안 보기
mkdir   폴더 만들기
touch   .md 파일 만들기
open    .md 파일 열기
save    .md 파일 저장하기
mv      이동 또는 이름 변경
rm      삭제
find    파일/폴더 이름과 경로 검색
grep    .md 본문 검색
```

`cd`, `pwd`, `tree`, `cp`, `restore`는 1차 범위에서 제외한다. 나중에
command input이 강해질 때 추가한다.

## 핵심 모델

### Workspace

workspace는 사용자의 개인 vault 경계다.

클라이언트는 `workspace_id`를 보내지 않는다. API는 인증된 사용자로부터
workspace를 결정한다. 모든 조회와 변경은 이 workspace 안에서만 실행한다.

### Node

node는 파일 트리 안의 항목 하나다.

```text
kind = folder
kind = document
```

폴더와 문서는 같은 트리 안에 같이 보이므로 둘 다 `nodes`에 저장한다.

최소 필드:

```text
id
workspace_id
parent_id
name
kind
path_cache
sort_order
created_at
updated_at
deleted_at
```

`parent_id`가 트리 구조를 만든다.

```text
id    parent_id    name        kind
--------------------------------------
1     null         /           folder
2     1            projects    folder
3     2            memgrep.md  document
```

### Document

document는 document node 하나의 Markdown 본문을 저장한다.

`kind = document`인 node만 `documents` row를 가진다.

최소 필드:

```text
node_id
workspace_id
content_md
search_text
created_at
updated_at
```

folder node는 document row를 가지지 않는다.

### Node ID와 Path

`node_id`는 정체성이다. `path_cache`는 사용자에게 보이는 경로다.

```text
node_id: 4d9...
path:    /projects/memgrep.md
```

이름을 바꾸거나 이동하면 path는 바뀌지만 node identity는 유지된다.

구조화된 UI는 node ID를 사용한다. command input은 path를 받을 수 있지만,
서버는 path를 node ID로 해석한 뒤 같은 core operation을 호출한다.

## API 설계

### Root 조회

현재 workspace의 root folder node를 가져온다.

```http
GET /api/v1/vault/root
```

응답:

```json
{
  "node": {
    "id": "root-node-id",
    "parent_id": null,
    "name": "/",
    "kind": "folder",
    "path": "/",
    "has_children": true
  }
}
```

### Path 해석

command input이나 URL 기반 동작에서 path를 node로 해석한다.

```http
GET /api/v1/vault/resolve?path=/projects/memgrep.md
```

응답:

```json
{
  "node": {
    "id": "doc-node-id",
    "parent_id": "projects-node-id",
    "name": "memgrep.md",
    "kind": "document",
    "path": "/projects/memgrep.md",
    "has_children": false
  }
}
```

규칙:

- root 밖의 path는 허용하지 않는다.
- 삭제된 node는 해석하지 않는다.
- 같은 workspace 안에서만 해석한다.

path는 서버에서 canonical form으로 정규화한 뒤 해석한다.

정규화 규칙:

- path는 반드시 `/`로 시작한다.
- root는 `/`만 허용한다.
- root 외 trailing slash는 제거한다.
- 중복 slash는 거부하거나 하나로 정규화한다.
- `.` segment는 거부하거나 제거한다.
- `..` segment는 거부한다.
- 정규화된 path만 `path_cache` 조회에 사용한다.

### ls

폴더의 직접 자식만 가져온다.

```http
GET /api/v1/vault/nodes/{node_id}/children
```

규칙:

- `{node_id}`는 folder여야 한다.
- 직접 자식만 반환한다.
- 삭제된 node는 제외한다.

응답:

```json
{
  "parent": {
    "id": "projects-node-id",
    "path": "/projects"
  },
  "children": [
    {
      "id": "doc-node-id",
      "parent_id": "projects-node-id",
      "name": "memgrep.md",
      "kind": "document",
      "path": "/projects/memgrep.md",
      "has_children": false
    }
  ]
}
```

### mkdir

폴더를 만든다.

```http
POST /api/v1/vault/folders
```

요청:

```json
{
  "parent_node_id": "parent-folder-node-id",
  "name": "projects"
}
```

규칙:

- `parent_node_id`는 folder node여야 한다.
- 같은 parent 아래에 같은 이름의 살아있는 node가 있으면 실패한다.
- 생성된 folder node를 반환한다.

### touch

빈 `.md` 문서를 만든다.

```http
POST /api/v1/vault/documents
```

요청:

```json
{
  "parent_node_id": "parent-folder-node-id",
  "name": "memgrep.md"
}
```

규칙:

- `parent_node_id`는 folder node여야 한다.
- `name`은 `.md`로 끝나야 한다.
- 같은 parent 아래에 같은 이름의 살아있는 node가 있으면 실패한다.
- `nodes` row와 `documents` row를 하나의 transaction에서 만든다.
- 초기 `content_md`는 빈 문자열이다.

생성 동작:

1. `kind = document`인 `nodes` row를 만든다.
2. 생성된 `node_id`에 연결된 `documents` row를 만든다.
3. 새 document node를 반환한다.

### open

문서 본문을 읽는다.

```http
GET /api/v1/vault/documents/{node_id}
```

규칙:

- `{node_id}`는 document node여야 한다.
- 삭제된 node는 열 수 없다.

응답:

```json
{
  "node": {
    "id": "doc-node-id",
    "name": "memgrep.md",
    "kind": "document",
    "path": "/projects/memgrep.md"
  },
  "document": {
    "node_id": "doc-node-id",
    "content_md": "# memgrep\n"
  }
}
```

### save

문서 본문을 저장한다.

```http
PATCH /api/v1/vault/documents/{node_id}
```

요청:

```json
{
  "content_md": "# memgrep\nUpdated content.\n"
}
```

동작:

- `documents.content_md`를 갱신한다.
- `documents.search_text`를 갱신한다.
- `documents.updated_at`을 갱신한다.
- `nodes.updated_at`을 갱신한다.

### mv

node를 이동하거나 이름을 바꾼다.

```http
PATCH /api/v1/vault/nodes/{node_id}/move
```

요청:

```json
{
  "new_parent_node_id": "archive-folder-node-id",
  "new_name": "memgrep.md"
}
```

규칙:

- root는 이동하거나 이름을 바꿀 수 없다.
- `{node_id}`는 이동할 folder 또는 document다.
- `new_parent_node_id`는 folder node여야 한다.
- `new_name`은 선택이다. 없으면 기존 이름을 유지한다.
- document의 `new_name`은 소문자 `.md`로 끝나야 한다.
- folder의 `new_name`은 `.md`로 끝날 수 없다.
- 자기 자신이나 자신의 하위 폴더로 이동할 수 없다.
- 같은 target parent 아래에 같은 이름의 살아있는 node가 있으면 실패한다.
- 이동/이름 변경 후 `path_cache`를 갱신한다.
- folder를 이동/이름 변경하면 모든 descendant의 `path_cache`도 갱신한다.

예시:

```text
mv /projects/a.md /projects/b.md
  -> same parent, new_name = b.md

mv /projects/a.md /archive/a.md
  -> new_parent_node_id = /archive, same name

mv /projects/a.md /archive/b.md
  -> new_parent_node_id = /archive, new_name = b.md
```

### rm

node를 삭제한다.

```http
DELETE /api/v1/vault/nodes/{node_id}
```

규칙:

- root는 삭제할 수 없다.
- soft delete한다.
- document 삭제는 해당 document node를 삭제한다.
- folder 삭제는 folder와 모든 descendant를 함께 soft delete한다.
- 삭제된 node는 `ls`, `find`, `grep`, `resolve`에서 보이지 않는다.

## 검색

### find

파일/폴더 이름과 경로를 검색한다. Markdown 본문은 보지 않는다.

```http
POST /api/v1/vault/search/find
```

요청:

```json
{
  "q": "memgrep",
  "path": "/projects",
  "kind": "document",
  "limit": 50
}
```

규칙:

- `nodes`만 검색한다.
- 현재 workspace 안에서만 검색한다.
- `deleted_at IS NULL`인 node만 검색한다.
- `kind`는 선택이며 `folder` 또는 `document`다.
- `path`가 있으면 해당 subtree로 제한한다.
- 검색어를 URL query string에 남기지 않기 위해 POST body를 사용한다.
- `limit` 기본값은 50이다.

### grep

Markdown 본문을 검색한다.

```http
POST /api/v1/vault/search/grep
```

요청:

```json
{
  "q": "memgrep",
  "path": "/projects",
  "context": 2,
  "limit": 50
}
```

동작:

1. `documents.search_text`로 후보 문서를 찾는다.
2. `nodes`와 join해서 workspace, path, deleted filter를 적용한다.
3. 애플리케이션 코드에서 `content_md`를 줄 단위로 나눈다.
4. 매칭된 line number와 선택적 context를 반환한다.

guard:

- `limit` 기본값은 50이다.
- `context` 최대값은 5다.
- 너무 짧은 `q`는 거부할 수 있다.
- 전체 match 수는 서버에서 제한한다.

응답:

```json
{
  "results": [
    {
      "node_id": "doc-node-id",
      "path": "/projects/memgrep.md",
      "line_no": 12,
      "line": "memgrep stores Markdown notes in a small vault.",
      "before": [],
      "after": []
    }
  ]
}
```

## Command 매핑

command input은 구조화된 API 위에 얹는다.

command input은 core API를 대체하지 않는다. 버튼 UI, command input,
나중의 AI tool call은 모두 같은 API를 사용해야 한다.

```text
ls /projects
  -> GET /api/v1/vault/resolve?path=/projects
  -> GET /api/v1/vault/nodes/{node_id}/children

mkdir /projects
  -> GET /api/v1/vault/resolve?path=/
  -> POST /api/v1/vault/folders

touch /projects/memgrep.md
  -> GET /api/v1/vault/resolve?path=/projects
  -> POST /api/v1/vault/documents

open /projects/memgrep.md
  -> GET /api/v1/vault/resolve?path=/projects/memgrep.md
  -> GET /api/v1/vault/documents/{node_id}

save /projects/memgrep.md
  -> GET /api/v1/vault/resolve?path=/projects/memgrep.md
  -> PATCH /api/v1/vault/documents/{node_id}

mv /projects/a.md /archive/b.md
  -> GET /api/v1/vault/resolve?path=/projects/a.md
  -> GET /api/v1/vault/resolve?path=/archive
  -> PATCH /api/v1/vault/nodes/{node_id}/move

rm /projects/memgrep.md
  -> GET /api/v1/vault/resolve?path=/projects/memgrep.md
  -> DELETE /api/v1/vault/nodes/{node_id}

find memgrep
  -> POST /api/v1/vault/search/find

grep memgrep /projects
  -> POST /api/v1/vault/search/grep
```

## 이름 규칙

root를 제외한 node name은 다음 규칙을 따른다.

- 빈 문자열일 수 없다.
- `/`를 포함할 수 없다.
- `.` 또는 `..`일 수 없다.
- document node는 소문자 `.md`로 끝나야 한다.
- folder node는 `.md`로 끝날 수 없다.

DB CHECK는 최소 방어선이다. 실제 검증과 에러 메시지는 application layer에서
처리한다.

## 에러 정책

- 인증되지 않은 요청은 `401`을 반환한다.
- 존재하지 않는 node/path는 `404`를 반환한다.
- 다른 사용자의 workspace에 속한 리소스 접근은 `404`처럼 처리한다.
- folder가 필요한데 document인 경우 `400`을 반환한다.
- document가 필요한데 folder인 경우 `400`을 반환한다.
- 이름 중복은 `409`를 반환한다.
- 잘못된 파일명은 `400`을 반환한다.
- `.md`가 아닌 document 이름은 `400`을 반환한다.
- root 삭제, 이동, 이름 변경 시도는 `409`를 반환한다.
- 자기 자신 또는 descendant 아래로 이동하려는 시도는 `409`를 반환한다.

## 불변식

- 모든 operation은 인증된 사용자의 workspace를 사용한다.
- 클라이언트는 `workspace_id`를 보내지 않는다.
- root 밖의 path는 접근할 수 없다.
- path는 서버에서 canonical form으로 정규화한다.
- node는 정확히 하나의 workspace에 속한다.
- parent node는 반드시 같은 workspace의 folder node여야 한다.
- document row는 연결된 node와 같은 workspace에 속해야 한다.
- document node만 document row를 가진다.
- folder node는 document row를 가지지 않는다.
- document node 이름은 소문자 `.md`로 끝난다.
- folder node 이름은 `.md`로 끝날 수 없다.
- children API는 직접 자식만 반환한다.
- `node_id`는 정체성이다.
- `path_cache`는 표시와 검색을 위한 cache다.
- move/rename은 영향을 받는 하위 path도 갱신해야 한다.
- 삭제된 node는 `ls`, `find`, `grep`, `resolve`에서 보이지 않는다.

## 구현 순서

먼저 아래 순서로 실제 동작을 뚫는다.

```text
authgate 연동
-> current user 확인
-> default workspace/root 초기화
-> root
-> ls
-> mkdir
-> touch
-> open
-> save
```

그 다음 `mv`, `rm`, `find`, `grep`을 붙인다.
