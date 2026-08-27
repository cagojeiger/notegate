# MCP tools

MCP는 User·Agent client용 target-first path API다. Tool은 파일 시스템 명령처럼 동작하고 Space lifecycle은 REST/dashboard가 담당한다. 독립 조회는 `run_read_sequence`, 순차 mutation은 `run_write_sequence`로 묶을 수 있다.

```text
target = space:/absolute/path
```

Space name은 Unicode를 허용하지만 `target` 파싱을 위해 `:`는 사용할 수 없다. `target`의 Space name은 exact match이며 대소문자를 구분한다.

노출되는 tool은 다음 9개다.

```text
me      caller identity/server version 확인
read    spaces/ls/tree/stat/read/changes
search  find/grep
write   write/append/patch/edit
manage  mkdir/mv/cp/rm
file_download  presigned GET 준비
file_upload    begin_upload/prepare_parts/complete_upload/abort_upload
run_read_sequence   bounded concurrent read/search
run_write_sequence  ordered fail-fast write/manage
```

- `me`는 입력이 없다.
- 나머지 tool은 앞뒤 공백 없는 1..200자의 `purpose`가 필수다.
- sequence tool은 sequence 전체에 하나의 `purpose`를 지정한다.
- 인증된 호출의 실행 이력은 민감한 원문을 저장하지 않는 별도 snapshot으로 기록한다.

수집 경계, redaction, sequence 집계와 보존 기간은 [`event-logging.md`](../event-logging.md#command-invocation-history)를 따른다.

## 버전 확인

NoteGate release version과 MCP protocol version은 독립적이다.

| 대상 | 확인 방법 |
|---|---|
| Source release | repository root의 [`VERSION`](../../../VERSION) |
| 실행 중인 server release | 응답 `_meta.io.modelcontextprotocol/serverInfo.version` 또는 `me.server_version` |
| MCP `2026-07-28` | `server/discover.supportedVersions`, 응답 `_meta.io.modelcontextprotocol/protocolVersion`, Streamable HTTP의 `MCP-Protocol-Version` |
| initialize 기반 protocol | `initialize.protocolVersion`, `initialize.serverInfo.version` |

문서/소스와 실행 중인 서버를 비교할 때는 `VERSION`, `me.server_version`, client에 노출된 tool 목록을 함께 확인한다. Tool 목록이 다르면 client/connector의 schema cache와 구독 상태를 확인한다.

## Tool 목록 갱신

MCP `2026-07-28`은 `tools.listChanged=true`를 제공한다. 일반 MCP POST는 stateless JSON request/response이고, `subscriptions/listen`은 tool 목록 변경을 전달하는 장기 SSE 응답이다.

구독이 성립하면 서버는 최초 `notifications/tools/list_changed`를 전송한다.

- Client는 알림을 받으면 현재 endpoint의 `tools/list`를 다시 호출해 cache한 schema와 description을 교체한다.
- 파드가 종료되면 활성 구독도 종료된다. Client는 새 파드에 연결해 구독을 다시 연다.
- `tools/list`의 `ttlMs`는 5분이고 `cacheScope=public`이다.
- 알림 구독을 지원하지 않는 client는 연결을 다시 만들고 `tools/list`를 재호출한다.

REST/dashboard user API가 Space lifecycle, agent와 API key 관리를 담당한다.

변경 기록은 `read op=changes` 하나의 Space-root mutation stream이다. 다른 paginated read와 동일하게 `cursor`와 `page.next_cursor`를 사용하며, `direction`은 최신에서 과거로 읽는 `older`(기본값) 또는 checkpoint 이후를 적용 순서로 읽는 `newer`다.

## 세부 계약

- 인증: [`auth.md`](./auth.md)
- caller identity: [`identity.md`](./identity.md)
- Space scope: [`spaces.md`](./spaces.md)
- Tool input/output: [`tools.md`](./tools.md)
- File entry point: [`files.md`](./files.md)
- Search entry point: [`search.md`](./search.md)
