# NoteGate CLI

`notegate-cli`는 사람과 AI agent가 MCP transport 없이 NoteGate의 공통 Command API를 호출하는 JSON CLI다. 현재 수직 흐름은 Agent API key 기반 `me`와 `read`만 제공한다.

## 인증과 연결

```sh
export NOTEGATE_BASE_URL='https://<notegate-host>'
export NOTEGATE_API_KEY='ngk_v2_...'
```

- `NOTEGATE_API_KEY`는 Agent가 소유한 `ngk_v2_` key여야 한다.
- API key는 shell history와 process list 노출을 막기 위해 command-line option으로 받지 않는다.
- `--base-url`은 `NOTEGATE_BASE_URL`보다 우선한다.
- 원격 origin은 HTTPS만 허용한다. 로컬 개발용 HTTP는 `localhost`와 loopback IP에서만 허용한다.
- 현재 credential 저장, User Device Flow와 token refresh는 제공하지 않는다.
- HTTP redirect를 따르지 않으므로 bearer credential이 다른 origin으로 전달되지 않는다.

## 명령

```sh
notegate-cli me

notegate-cli read \
  --input '{"purpose":"list connected spaces","op":"spaces"}'

notegate-cli read --input-file request.json
printf '%s' '{"purpose":"list connected spaces","op":"spaces"}' \
  | notegate-cli read --input-file -
```

`read` JSON은 MCP와 Command API의 공통 `ReadInput`을 그대로 사용한다. `--schema`는 같은 Rust type에서 생성된 JSON Schema를 출력하므로 별도의 CLI 필드 정의가 없다.

```sh
notegate-cli read --schema
notegate-cli read --help
```

## 출력 계약

- 성공 JSON 또는 `--schema`는 stdout에 한 줄 JSON으로 출력한다.
- 서버 오류 JSON은 `next_action`을 포함해 수정하지 않고 stderr로 전달한다.
- CLI configuration, local input, network와 protocol 오류도 stderr에 JSON으로 출력한다.
- help와 version은 clap의 일반 text 형식을 사용하고, argument parser 오류는 `invalid_arguments` JSON으로 출력한다.

| Exit code | 의미 |
|---:|---|
| `0` | 성공 또는 help/schema 출력 |
| `2` | CLI configuration, argument 또는 local JSON input 오류 |
| `3` | 인증 또는 권한 오류 |
| `4` | NoteGate가 command를 거부한 비재시도 오류 |
| `5` | rate limit, 서버/네트워크, timeout 또는 protocol 오류 |

기본 timeout은 30초이며 `--timeout-seconds` 또는 `NOTEGATE_TIMEOUT_SECONDS`로 1~300초 사이에서 설정한다. 입력은 1 MiB, 응답은 8 MiB로 제한한다.

## 현재 제외 범위

- `search`, `write`, `manage`, `file_upload`, `file_download`
- `run_sequence`
- API key 영구 저장과 profile
- User Device Flow
- 설치용 플랫폼별 binary artifact

Command API의 서버 계약은 [`command-api.md`](./command-api.md)를 따른다.
