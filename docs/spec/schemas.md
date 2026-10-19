# Shared schemas

이 문서는 node/text/file surface가 공유하는 응답 구조를 정의한다. REST는 UI 화면 렌더링을 위해 id-first의 넓은 resource shape를 반환하고, MCP는 agent 호출을 위해 path-first의 작은 tool shape를 반환한다.

## Page

필드와 직렬화 형태는 [Page](../../backend/crates/api/src/page.rs)를 따른다.

`next_cursor`는 opaque string이다. Client는 해석하지 않고 다음 요청에 그대로 전달한다.

## Target

MCP `read`/`search`/`write`/`manage` tool과 sequence `commands[]`가 사용하는 대상 지정자다.

필드와 직렬화 형태는 [command input](../../backend/crates/command/src/input.rs)를 따른다.

MCP는 `target` 하나로 Space와 path를 함께 지정한다. Root는 `"space:/"`다.

## NodeKind

필드와 직렬화 형태는 [NodeKind](../../backend/crates/model/src/node.rs)를 따른다.

## AccountRef

필드와 직렬화 형태는 [AccountRef](../../backend/crates/model/src/account.rs)를 따른다.

## NodeRef

REST Text response처럼 이미 선택된 node를 짧게 참조할 때 사용한다.

필드와 직렬화 형태는 [NodeRef](../../backend/crates/api/src/rest/dto.rs)를 따른다.

## RestNode

REST node/file endpoints가 반환하는 UI용 node resource shape다. Content body는 포함하지 않는다.

필드와 직렬화 형태는 [NodeOut](../../backend/crates/api/src/rest/dto.rs)를 따른다.

`byte_len`은 Text에서는 저장된 text payload 기준이고 File에서는 저장 bytes 기준이다. Folder에는 `byte_len`이 없다. `content_sha256`은 Text에만 존재한다.

`write_locked`는 현재 node에 직접 설정된 값이다. `effective_write_locked`는 현재 node 또는 live 조상에 직접 잠금이 하나라도 있는지 나타낸다. 상세 REST resource의 `write_lock_sources`는 그 직접 잠금 source를 root부터 나열한다. Tree/Recent용 `NodeSummary`는 원인 목록 없이 잠긴 경우에만 `effective_write_locked: true`를 포함하며, 생략된 값은 `false`다.

MCP/API v2는 외부 접근이 꺼진 잠금 source의 상세 정보를 제외하지만 `effective_write_locked`는 유지한다. 브라우저 응답은 전체 잠금 source를 제공한다.

## REST node list/reveal envelopes

필드와 직렬화 형태는 [node responses](../../backend/crates/api/src/rest/nodes.rs)를 따른다.

`RestNodeRevealResponse.ancestors`는 root부터 target parent까지의 chain이다. Target node는 `target` 필드에만 들어간다.

## McpNodeSummary

MCP `read`, `search`, `write`, `manage` 결과가 반환하는 path-first node summary다. Content body와 node metadata는 포함하지 않는다.

필드와 optional 여부는 [PathNodeSummary](../../backend/crates/api/src/path_node_summary.rs)가 정의한다. `write_locked`는 노드 자체 설정, `effective_write_locked`는 상위 경로를 포함한 유효 잠금 상태다. Text/file별 선택적 속성과 직렬화 검증도 같은 소스를 따른다.

MCP는 File metadata/stat을 읽지만 File bytes는 전송하지 않는다.

## McpGrepSummary

`search op=grep` item은 `McpNodeSummary`에 선택적 line match 정보를 더한다. `match_lines`는 `lines="first"` 또는 `lines="all"`일 때만 존재한다.

필드와 직렬화 형태는 [search response mapping](../../backend/crates/api/src/commands/search.rs)를 따른다.

## Text read shapes

REST Text read는 `{ node: NodeRef, text: ... }` envelope를 사용한다. MCP `read op=read`는 path-first flat shape를 사용한다.

필드와 직렬화 형태는 [Text read responses](../../backend/crates/api/src/rest/text.rs)를 따른다.

필드와 직렬화 형태는 [command Text read output](../../backend/crates/api/src/commands/files.rs)를 따른다.

MCP는 client-side encrypted Text를 읽지 않는다. Client-side encrypted Text는 REST Text API에서만 저장/조회한다. 서버 관리 방식으로 at-rest 암호화된 plain Text는 일반 plain Text와 같은 shape로 반환한다.
