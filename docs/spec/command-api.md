# CLI transport

`POST /cli`는 `notegate-cli`가 MCP와 같은 command contract를 JSON HTTP로 호출하는 단일 transport endpoint다. MCP와 CLI는 도구 이름, input schema, `purpose`, 업무 검증, sequence 실행, recovery action을 공통 계층에서 파생한다. 차이는 MCP의 JSON-RPC envelope와 CLI의 HTTP envelope뿐이다.

별도의 URL API version은 두지 않는다. CLI와 NoteGate server는 명시적인 Command Protocol version으로 호환성을 판단하며, 호환되지 않는 조합은 실행 전에 구조화 오류로 중단한다. Package release version은 진단과 관측에만 사용한다.

## 인증

Agent가 소유한 `ngk_v2_` API key와 AuthGate Device Flow가 발급한 User OAuth access token을 허용한다.

```http
Authorization: Bearer ngk_v2_...
X-Notegate-CLI-Version: <CLI release version>
X-Notegate-Command-Protocol: 1
```

User OAuth token의 `aud`는 서버의 `NOTEGATE_CLI_OAUTH_CLIENT_ID`와 정확히 일치해야 한다. Browser session cookie, MCP resource audience token과 legacy `ngk_v1_` key는 허용하지 않는다. 반대로 User MCP는 계속 `NOTEGATE_RESOURCE_URL` audience만 허용하고 Public V2는 Agent `ngk_v2_` key만 허용한다. 모든 응답은 `Cache-Control: private, no-store`다.

공식 CLI는 `X-Notegate-CLI-Version`을 진단용으로 항상 전송하지만 서버는 package version의 정확한 일치를 요구하지 않는다. `X-Notegate-Command-Protocol`은 필수이며, 현재 지원 version은 `1`이다. 이 header가 누락됐거나 지원되지 않으면 `426 Upgrade Required`, `error=cli_update_required`, `kind=client_protocol_incompatible`, `next_action={"kind":"run_command","command":"notegate-cli update"}`를 반환하며 command를 실행하지 않는다.

Patch release는 가능한 한 같은 Command Protocol을 유지한다. Protocol을 올릴 때는 rolling deployment에서 구·신 파드가 공존하는 기간을 고려해 호환 기간 또는 명시적인 배포 절차를 함께 정의해야 한다.

## Endpoint와 envelope

```http
POST /cli
Content-Type: application/json
```

```json
{
  "tool": "read",
  "input": {
    "purpose": "연결된 Space 목록 확인",
    "op": "spaces",
    "limit": 20
  }
}
```

허용되는 `tool`은 MCP와 정확히 같다.

| Tool | 공통 input |
|---|---|
| `me` | 빈 object `{}` |
| `read` | `ReadInput` |
| `search` | `SearchInput` |
| `write` | `WriteInput` |
| `manage` | `ManageInput` |
| `file_download` | `FileDownloadInput` |
| `file_upload` | `FileUploadInput` |
| `run_read_sequence` | `RunReadSequenceInput` |
| `run_write_sequence` | `RunWriteSequenceInput` |

`me`만 `purpose` 예외다. 다른 모든 직접 도구는 자기 input에 `purpose`가 필요하다. Sequence는 top-level `purpose`를 한 번만 받고, 내부 command에는 `purpose`나 `args` wrapper를 넣지 않는다.

- `run_read_sequence`: read/search 1..20개, 최대 4개 병렬 실행, 결과는 입력 순서
- `run_write_sequence`: write/manage 1..20개, 입력 순서 직렬 실행, 첫 실패 후 중단, rollback 없음
- 두 sequence 모두 모든 command의 정적으로 검증 가능한 오류를 실행 전에 모아 반환한다.

성공 시 공통 command 결과를 추가 envelope 없이 반환한다. Path, pagination, Text/File, search와 write-lock semantics는 [`files-commands.md`](./files-commands.md)를 따른다.

## 오류

HTTP status는 transport 결과를 나타내고 JSON body는 MCP와 같은 안정 code와 recovery metadata를 보존한다.

```json
{
  "error": "required_field_missing",
  "kind": "invalid_input",
  "message": "target is required",
  "data": {
    "code": "required_field_missing",
    "retryable": false,
    "recoverable": true,
    "next_action": {
      "kind": "add_fields",
      "fields": [{"field": "target"}]
    }
  }
}
```

클라이언트는 자연어 `message` 대신 `error`, `kind`, `data.retryable`, `data.next_action`으로 분기한다. 잘못된 JSON, 허용되지 않은 method, body limit, timeout과 rate limit도 JSON 오류로 정규화한다.

## 호출 이력과 메트릭

인증을 통과해 `/cli`에 도달한 요청은 `surface=cli`인 command invocation 한 행으로 best-effort 기록한다. CLI argument/local file 오류와 서버에 도달하지 못한 network 실패는 포함하지 않는다. Sequence는 top-level 한 행으로 기록하며 내부 command별 행을 만들지 않는다.

Prometheus command metric은 MCP와 같은 family를 사용하고 `surface=cli`로 분리한다. 기록 범위, redaction과 retention은 [`event-logging.md`](./event-logging.md#command-invocation-history), metric 계약은 [`observability.md`](./observability.md#command-invocation-metrics)를 따른다.

## 운영 한계

- Public V2와 같은 Agent API rate-limit budget을 사용한다.
- 모든 요청은 공통 ingress body limit와 deadline을 적용받는다.
- 실제 File bytes는 JSON body를 통과하지 않고 presigned URL로 object storage와 직접 전송한다.
- 호출 이력 저장 실패는 이미 수행된 command 결과를 실패로 바꾸지 않는다.
