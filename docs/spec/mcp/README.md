# MCP tools

MCP는 agent/CLI용 target-first path API다. Tool은 파일 시스템 명령처럼 동작하되, Space lifecycle은 다루지 않는다. 여러 명령을 순서대로 실행할 때는 `run_sequence`를 사용한다.

```text
target = space:/absolute/path
```

Space name은 Unicode를 허용하지만 `target` 파싱을 위해 `:`는 사용할 수 없다. `target`의 Space name은 exact match이며 대소문자를 구분한다.

노출되는 tool은 다음 7개다.

```text
me      caller identity/server version 확인
read    spaces/ls/tree/stat/read/changes
search  find/grep
write   write/append/patch/edit
manage  mkdir/mv/cp/rm
file_transfer  begin_upload/prepare_parts/complete_upload/abort_upload/prepare_download
run_sequence  ordered command sequence 실행
```

`me`는 입력이 없다. 나머지 tool은 앞뒤 공백 없는 1..200자의 `purpose`가 필수이며, `run_sequence`는 sequence 전체에 하나만 지정한다. 서버는 인증된 `tools/call`마다 caller, tool/op, purpose, 원본 arguments JSON, 결과와 실행 시간을 별도 invocation history에 기록한다. 입력 schema나 purpose 검증에 실패한 호출도 기록하며, `run_sequence`는 내부 command별 행이 아니라 전체 sequence 한 행으로 남는다.

## 버전 확인

- 이 checkout의 source release version은 repository root의 [`VERSION`](../../../VERSION)이 정본이다.
- 실행 중인 서버 버전은 MCP `initialize` 응답의 `serverInfo.version` 또는 `me.server_version`으로 확인한다.
- MCP protocol version은 `initialize.protocolVersion`이며 NoteGate release version과 별개다.

문서/소스와 실행 중인 서버가 다른지 조사할 때는 `VERSION`, `me.server_version`, 현재 client에 노출된 tool 이름을 함께 기록한다. 서버 버전은 최신인데 tool 목록이 다르면 client/connector의 schema cache와 구독 상태를 확인한다.

## Tool 목록 갱신

서버는 MCP `2026-07-28`과 `tools.listChanged=true`를 지원한다. 일반 MCP POST는 세션 없는 JSON request/response를 유지하고, 갱신을 구독한 client의 `subscriptions/listen` 요청만 장기 SSE 응답으로 유지한다.

구독이 성립하면 서버는 최초 `notifications/tools/list_changed`를 전송한다. Client는 이 알림을 받으면 현재 endpoint에 `tools/list`를 다시 호출해 설치 시점 또는 이전 연결에서 보관한 schema와 description을 교체해야 한다. 파드가 종료되면 서버는 활성 구독을 정상 종료하며, client는 새 파드에 연결해 구독을 다시 열어야 한다. `tools/list`의 `ttlMs`는 5분이고 `cacheScope=public`이다. 알림 구독을 지원하지 않는 구형 client는 연결을 다시 만들고 `tools/list`를 재호출해야 한다.

MCP는 space create/delete/rename, agent 관리, API key 관리를 제공하지 않는다. 이 작업은 REST/dashboard user-only API에서 한다.

변경 기록은 `read op=changes` 하나의 Space-root mutation stream이다. 다른 paginated read와 동일하게 `cursor`와 `page.next_cursor`를 사용하며, `direction`은 최신에서 과거로 읽는 `older`(기본값) 또는 checkpoint 이후를 적용 순서로 읽는 `newer`다.

정본 tool contract는 [`tools.md`](./tools.md)를 따른다.
