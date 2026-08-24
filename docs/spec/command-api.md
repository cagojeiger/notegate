# Command API

Command API는 `notegate-cli` 같은 HTTP client가 NoteGate의 공통 path-first command engine을 직접 호출하는 transport다. MCP와 입력·업무 검증·권한·복구 action을 공유하지만 JSON-RPC envelope나 MCP sequence를 제공하지 않는다.

## 인증

현재 버전은 Agent가 소유한 `ngk_v2_` API key만 허용한다.

```http
Authorization: Bearer ngk_v2_...
```

Browser session cookie, OAuth bearer와 legacy `ngk_v1_` key는 허용하지 않는다. Device Flow 기반 User 인증은 별도 단계이며 현재 계약에 포함하지 않는다. 모든 응답은 `Cache-Control: private, no-store`다.

## Endpoint

| Method | Path | 공통 command |
|---|---|---|
| `GET` | `/api/commands/v1/me` | identity |
| `POST` | `/api/commands/v1/read` | read |
| `POST` | `/api/commands/v1/search` | search |
| `POST` | `/api/commands/v1/write` | write |
| `POST` | `/api/commands/v1/manage` | manage |
| `POST` | `/api/commands/v1/file_upload` | file upload lifecycle |
| `POST` | `/api/commands/v1/file_download` | presigned download |

`run_sequence`는 제공하지 않는다. CLI가 여러 명령을 실행할 때 각 endpoint를 명시적으로 호출한다.

## 요청과 성공 응답

`me`를 제외한 요청은 공통 command input과 동일한 JSON object이며 `purpose`가 필수다.

```json
{
  "purpose": "연결된 Space 목록 확인",
  "op": "spaces",
  "limit": 20
}
```

성공 시 공통 command 결과를 별도 envelope 없이 JSON으로 반환한다. Path, pagination, Text/File, search와 write-lock semantics는 [`files-commands.md`](./files-commands.md)를 따른다.

## 오류

HTTP status는 오류 종류를 나타내고 JSON body는 LLM과 CLI가 재시도 여부 및 수정 방법을 결정하는 구조화 정보를 보존한다.

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
      "fields": [
        { "field": "target" }
      ]
    }
  }
}
```

`error`는 가장 구체적인 안정 code이고 `kind`는 상위 오류 분류다. `data.code`는 `error`와 같은 code를 유지하며 recovery metadata와 함께 공통 command 결과를 그대로 보존한다. 클라이언트는 자연어 `message` 대신 `error`, `kind`, `data.retryable`, `data.next_action`으로 분기한다. 잘못된 JSON, 허용되지 않은 HTTP method, body limit, timeout과 rate limit도 JSON 오류로 정규화한다.

## 운영 한계

- Public V2와 같은 Agent API rate-limit budget을 사용한다.
- 모든 요청은 공통 ingress body limit와 deadline을 적용받는다.
- 실제 File bytes는 JSON body를 통과하지 않고 presigned URL로 object storage와 직접 전송한다.
