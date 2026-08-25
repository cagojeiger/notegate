# NoteGate CLI

`notegate-cli`는 사람과 AI agent가 MCP transport 없이 NoteGate의 공통 Command API를 호출하는 JSON CLI다. `me`, `read`, `write`, `manage`는 User Device credential 또는 Agent API key를 bearer로 사용한다.

## 인증과 연결

```sh
export NOTEGATE_BASE_URL='https://<notegate-host>'

# User credential
notegate-cli auth login

# 또는 Agent credential
export NOTEGATE_API_KEY='ngk_v2_...'
```

- `auth login`은 NoteGate의 `/.well-known/oauth-authorization-server`에서 production AuthGate endpoint와 환경별 `cli_client_id`를 discovery한 뒤 RFC 8628 Device Flow를 시작한다.
- 로컬 NoteGate는 `notegate-cli-local`, 운영 NoteGate는 `notegate-cli` client를 광고하므로 같은 production AuthGate 계정을 사용해도 token audience와 저장 credential이 분리된다.
- `NOTEGATE_API_KEY`는 Agent가 소유한 `ngk_v2_` key여야 하며, 설정되어 있으면 일반 command에서 User credential보다 항상 우선한다.
- API key는 shell history와 process list 노출을 막기 위해 command-line option으로 받지 않는다.
- `--base-url`은 `NOTEGATE_BASE_URL`보다 우선한다.
- 원격 origin은 HTTPS만 허용한다. 로컬 개발용 HTTP는 `localhost`와 loopback IP에서만 허용한다.
- User access/refresh token은 versioned bundle로 OS keychain에 저장한다. keychain key는 `issuer + client_id`이고 bundle은 NoteGate `base_url`을 포함한다.
- access token 만료 60초 전부터 자동 refresh한다. process 간 file lock을 획득한 뒤 credential을 다시 읽고, write-ahead in-progress marker를 먼저 기록한 다음 refresh token rotation을 한 번만 수행한다. 정상적인 process 종료/crash 뒤 marker가 남으면 다음 실행은 구 token을 재사용하지 않는다. Unix에서는 marker file과 parent directory를 모두 sync하며, 다른 platform의 갑작스러운 전원 손실 durability는 OS와 filesystem 보장 범위를 따른다.
- refresh 응답이 timeout, body 손상 또는 성공 응답 저장 실패로 불명확하면 자동 재시도하지 않는다. credential을 안전 상태로 표시하거나 삭제하고 `auth login`을 요구한다.
- HTTP redirect를 따르지 않으므로 bearer credential이 다른 origin으로 전달되지 않는다.

진단용으로만 metadata discovery를 우회할 수 있다. 두 환경 변수를 반드시 함께 설정해야 한다.

```sh
export NOTEGATE_AUTHGATE_URL='https://authgate.project-jelly.io'
export NOTEGATE_CLI_CLIENT_ID='notegate-cli-local'
```

## User 인증 명령

```sh
notegate-cli auth login
notegate-cli auth status
notegate-cli auth logout
```

- `auth login`은 기존 User credential이 있으면 덮어쓰지 않고 `already_authenticated`를 반환한다. 먼저 `auth logout`으로 기존 refresh token을 폐기해야 한다.
- 같은 OAuth client의 인증 작업이 이미 lock을 보유하고 있으면 기다리지 않고 retryable `login_in_progress`를 반환한다. 진행 중인 인증이 끝난 뒤 `auth status`로 결과를 확인한다.
- 이전 refresh 결과가 불명확한 상태라면 `auth login`이 같은 credential lock 아래에서 구 local bundle과 marker를 삭제하고 새 Device Flow를 시작한다. 불명확한 구 refresh token을 revoke/refresh 요청에 다시 보내지 않는다.
- 새 login credential의 keychain write 뒤 profile index commit과 보상 삭제가 모두 실패하면 `credential_store_state_unknown`을 반환한다. 이때 위의 explicit AuthGate URL/client ID 두 override를 설정한 `auth logout`으로 issuer+client key를 직접 정리한 뒤 다시 로그인한다.
- `auth status`는 network 요청이나 refresh 없이 local 상태만 읽는다. `NOTEGATE_API_KEY`가 있으면 실제 일반 command 우선순위에 맞춰 `credential=agent_api_key`, `source=environment`를 표시하며 값은 출력하지 않는다.
- `auth logout`은 User refresh token revoke를 한 번 시도한 후 결과와 무관하게 local User credential을 삭제한다. `NOTEGATE_API_KEY`는 환경 변수이므로 삭제하지 않으며, 설정되어 있으면 결과에 unset 안내를 포함한다.

## 명령

```sh
notegate-cli me

notegate-cli read \
  --input '{"purpose":"list connected spaces","op":"spaces"}'

notegate-cli read --input-file request.json
printf '%s' '{"purpose":"list connected spaces","op":"spaces"}' \
  | notegate-cli read --input-file -

notegate-cli write \
  --input '{"purpose":"create note","op":"write","target":"daily:/notes/example.md","content":"hello","create":true}'

notegate-cli manage \
  --input '{"purpose":"create notes folder","op":"mkdir","target":"daily:/notes","parents":true}'
```

각 JSON 명령은 MCP와 Command API의 공통 `ReadInput`, `WriteInput`, `ManageInput`을 그대로 사용한다. `--schema`는 해당 Rust type에서 생성된 JSON Schema를 출력하므로 별도의 CLI 필드 정의가 없다.

```sh
notegate-cli read --schema
notegate-cli write --schema
notegate-cli manage --schema

notegate-cli read --help
notegate-cli write --help
notegate-cli manage --help
```

## 출력 계약

- 성공 JSON 또는 `--schema`는 stdout에 한 줄 JSON으로 출력한다.
- `auth login`만 polling 전에 아래 `verification_required` event를 stdout에 먼저 쓰고, 완료 시 `login_succeeded` event를 쓰는 NDJSON이다. `verification_uri_complete`는 `user_code`가 포함된 직접 인증 URL이며, issuer가 제공하지 않으면 CLI가 안전하게 구성한다. `device_code`, access token, refresh token과 ID token은 어떤 event나 error에도 포함하지 않는다.
- 서버 오류 JSON은 `next_action`을 포함해 수정하지 않고 stderr로 전달한다.
- CLI configuration, local input, network와 protocol 오류도 stderr에 JSON으로 출력한다.
- help와 version은 clap의 일반 text 형식을 사용하고, argument parser 오류는 `invalid_arguments` JSON으로 출력한다.

| Exit code | 의미 |
|---:|---|
| `0` | 성공 또는 help/schema 출력 |
| `2` | CLI configuration, argument 또는 local JSON input 오류 |
| `3` | 인증 또는 권한 오류 |
| `4` | NoteGate가 command를 거부한 비재시도 오류 |
| `5` | 일시적인 인증 경합, rate limit, 서버/네트워크, timeout 또는 protocol 오류 |

기본 timeout은 30초이며 `--timeout-seconds` 또는 `NOTEGATE_TIMEOUT_SECONDS`로 1~300초 사이에서 설정한다. 입력은 1 MiB, 응답은 8 MiB로 제한한다.

```json
{"event":"verification_required","verification_uri":"https://authgate.project-jelly.io/device","verification_uri_complete":"https://authgate.project-jelly.io/device?user_code=BCDF-GHKM","user_code":"BCDF-GHKM","expires_in":300,"interval":5}
{"event":"login_succeeded","base_url":"http://localhost:9191","issuer":"https://authgate.project-jelly.io","client_id":"notegate-cli-local","expires_at":1787530000}
```

## 현재 제외 범위

- `search`, `file_upload`, `file_download`
- `run_sequence`
- API key 영구 저장과 profile
- 설치용 플랫폼별 binary artifact

Command API의 서버 계약은 [`command-api.md`](./command-api.md)를 따른다.
