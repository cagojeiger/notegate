# NoteGate CLI

`notegate-cli`는 사람과 AI agent가 MCP transport 없이 같은 command contract를 호출하는 JSON CLI다. MCP와 동일한 9개 도구 이름과 input schema를 `POST /cli`로 전달하며 User Device credential 또는 Agent API key를 bearer로 사용한다. 서버 transport 계약은 [`command-api.md`](./command-api.md)를 따른다.

## 설치와 업데이트

공식 설치는 GitHub Release의 foreground shell installer를 사용한다. background daemon, launchd, systemd 등록은 하지 않는다.

```sh
curl -fsSL https://github.com/cagojeiger/notegate/releases/latest/download/notegate-cli-installer.sh | sh
export PATH="$HOME/.local/bin:$PATH"

# 설치 디렉터리 지정
curl -fsSL https://github.com/cagojeiger/notegate/releases/latest/download/notegate-cli-installer.sh | sh -s -- --bin-dir "$HOME/bin"
```

Installer는 shell rc 파일을 수정하지 않는다. 출력 JSON의 `path_on_path`가 `false`이면 `hint`에 나온 directory를 PATH에 추가해야 한다.

Release에는 네 platform의 단일 executable asset, 각 asset의 `.sha256`, `notegate-cli-manifest.json`, installer가 포함된다. Manifest는 `releases/download/v<version>/notegate-cli-manifest.json` 경로로 version-addressable하다.

| Target | Asset |
|---|---|
| macOS arm64 | `notegate-cli-aarch64-apple-darwin` |
| macOS x64 | `notegate-cli-x86_64-apple-darwin` |
| Linux arm64 | `notegate-cli-aarch64-unknown-linux-gnu` |
| Linux x64 | `notegate-cli-x86_64-unknown-linux-gnu` |

Installer는 `~/.local/bin/notegate-cli`에 설치하고 같은 directory에 `notegate-cli-install-receipt.json` receipt를 쓴다. `notegate-cli update`는 이 receipt가 현재 실행 파일과 일치할 때만 같은 directory 안에서 checksum 검증 후 atomic replace를 수행한다.

```sh
notegate-cli update --check
notegate-cli update
```

`update --check` 성공 출력은 `up_to_date` 또는 `update_available` JSON이고, `update` 성공 출력은 `updated` JSON이다. 수동 복사, source build, package manager 설치처럼 공식 installer receipt가 없는 실행 파일은 `unmanaged_install` configuration error를 반환한다.

## 인증과 연결

```sh
# User credential
notegate-cli auth login

# 또는 Agent credential
export NOTEGATE_API_KEY='ngk_v2_...'
```

- `auth login`은 NoteGate의 `/.well-known/oauth-authorization-server`에서 production AuthGate endpoint와 환경별 `cli_client_id`를 discovery한 뒤 RFC 8628 Device Flow를 시작한다.
- 로컬 NoteGate는 `notegate-cli-local`, 운영 NoteGate는 `notegate-cli` client를 광고하므로 같은 production AuthGate 계정을 사용해도 token audience와 저장 credential이 분리된다.
- `NOTEGATE_API_KEY`는 Agent가 소유한 `ngk_v2_` key여야 하며, 설정되어 있으면 일반 command에서 User credential보다 항상 우선한다.
- API key는 shell history와 process list 노출을 막기 위해 command-line option으로 받지 않는다.
- 기본 NoteGate origin은 `https://notegate.project-jelly.io`다. 다른 배포나 로컬 서버는 `NOTEGATE_BASE_URL` 또는 `--base-url`로 지정한다.
- 연결 우선순위는 `--base-url` > `NOTEGATE_BASE_URL` > 기본 운영 origin이다.
- 원격 origin은 HTTPS만 허용한다. 로컬 개발용 HTTP는 `localhost`와 loopback IP에서만 허용한다.

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

- `auth login`은 Device Flow를 시작한다. 저장된 User credential은 덮어쓰지 않고 `already_authenticated`를 반환하므로 먼저 `auth logout`으로 폐기한다.
- `auth status`는 network 요청이나 refresh 없이 local 상태를 읽는다. `NOTEGATE_API_KEY`가 있으면 `credential=agent_api_key`, `source=environment`를 표시하고 값은 숨긴다.
- `auth logout`은 User refresh token revoke를 한 번 시도한 뒤 local User credential을 삭제한다. 환경 변수인 `NOTEGATE_API_KEY`는 삭제하지 않고 unset 안내를 반환한다.

## 명령

```sh
notegate-cli me

notegate-cli read \
  --input '{"purpose":"list connected spaces","op":"spaces"}'

notegate-cli read --input-file request.json
notegate-cli read --all \
  --input '{"purpose":"read the complete note","op":"read","target":"daily:/notes/example.md"}'
printf '%s' '{"purpose":"list connected spaces","op":"spaces"}' \
  | notegate-cli read --input-file -

notegate-cli write \
  --input '{"purpose":"create note","op":"write","target":"daily:/notes/example.md","content":"hello","create":true}'

notegate-cli manage \
  --input '{"purpose":"create notes folder","op":"mkdir","target":"daily:/notes","parents":true}'

notegate-cli search \
  --input '{"purpose":"find notes","op":"find","target":"daily:/","q":"notes"}'

notegate-cli file_download \
  --input '{"purpose":"download report","target":"daily:/report.pdf"}'

notegate-cli run_read_sequence \
  --input '{"purpose":"inspect notes","commands":[{"tool":"read","op":"spaces"},{"tool":"search","op":"find","target":"daily:/","q":"notes"}]}'
```

CLI command surface는 `me`, `read`, `search`, `write`, `manage`, `file_download`, `file_upload`, `run_read_sequence`, `run_write_sequence`다. 각 JSON 명령은 MCP가 사용하는 동일한 공통 Rust input type을 그대로 사용한다. `--schema`는 그 type에서 생성된 JSON Schema를 출력하므로 별도의 CLI 필드 정의가 없다.

`read --all`은 `op=read` 전용 CLI 안전 옵션이다. 공통 command contract의 Text 상한을 한 번에 요청하고 `truncated`, byte 길이, 줄 수와 SHA-256을 모두 검증한 뒤에만 성공한다. CLI와 서버의 상한이 달라 완전성을 확인할 수 없으면 실패한다. `start_line`, `max_lines`, `max_bytes`, `if_none_match_sha256`와 함께 사용할 수 없다. 일반 범위 읽기가 `truncated=true`를 반환하면 `next_action`을 따라 마지막 page까지 읽어야 한다.

Sequence도 MCP와 같은 계약을 사용한다. `purpose`는 top-level에 한 번만 넣고 각 `commands[]`는 `tool`, `op`와 operation field를 가진 flat object다.

- `run_read_sequence`: read/search를 최대 4개 병렬 실행하고 결과를 입력 순서로 반환한다.
- `run_write_sequence`: write/manage를 순서대로 실행하며 첫 실패 뒤 남은 command를 건너뛴다.

```sh
notegate-cli read --schema
notegate-cli search --schema
notegate-cli write --schema
notegate-cli manage --schema
notegate-cli file_download --schema
notegate-cli file_upload --schema
notegate-cli run_read_sequence --schema
notegate-cli run_write_sequence --schema
notegate-cli update --help

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
- `update`는 NoteGate server나 AuthGate credential을 사용하지 않는다. GitHub Release manifest와 artifact checksum만 사용한다.
- 모든 command 요청은 진단용 CLI release version을 `X-Notegate-CLI-Version`, 호환성 계약을 `X-Notegate-Command-Protocol`로 보낸다. 서버는 package release가 아니라 Command Protocol로 호환성을 판단한다.
- Command Protocol이 누락됐거나 지원되지 않으면 command를 실행하지 않고 `cli_update_required`와 `notegate-cli update` action을 반환한다. CLI는 이 구조화 body를 stderr에 그대로 출력하고 exit `4`로 종료한다.

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

## 보안과 복구 세부 계약

- 서버에 도달한 각 command는 MCP와 같은 redaction·결과·보존 정책의 호출 이력으로 기록된다. History UI는 `surface=cli`를 MCP와 별도 tab으로 보여준다.
- CLI argument 오류, local file 오류와 서버에 도달하지 못한 network 실패는 서버 이력에 포함되지 않는다. 상세 보존 계약은 [`event-logging.md`](./event-logging.md#command-invocation-history)를 따른다.
- User access/refresh token은 versioned bundle로 OS keychain에 저장한다. keychain key는 `issuer + client_id`이고 bundle은 NoteGate `base_url`을 포함한다.
- HTTP redirect를 따르지 않으므로 bearer credential이 다른 origin으로 전달되지 않는다.
- access token 만료 60초 전부터 자동 refresh한다. process 간 file lock을 획득한 뒤 credential을 다시 읽고, write-ahead in-progress marker를 먼저 기록한 다음 refresh token rotation을 한 번만 수행한다.
- 정상적인 process 종료/crash 뒤 marker가 남으면 다음 실행은 구 token을 재사용하지 않는다. Unix에서는 marker file과 parent directory를 모두 sync하며, 다른 platform의 갑작스러운 전원 손실 durability는 OS와 filesystem 보장 범위를 따른다.
- refresh 응답이 timeout, body 손상, 성공 응답 저장 실패 또는 `invalid_grant` 외 OAuth 오류로 불명확하면 자동 재시도하지 않는다. credential을 안전 상태로 표시하거나 삭제하고 `auth login`을 요구한다.
- 같은 OAuth client의 인증 작업이 lock을 보유하면 `auth login`은 retryable `login_in_progress`를 반환한다. 작업 종료 뒤 `auth status`로 결과를 확인한다.
- Refresh 결과가 불명확한 상태라면 `auth login`이 같은 credential lock 아래에서 local bundle과 marker를 삭제하고 새 Device Flow를 시작한다. 해당 refresh token은 revoke/refresh 요청에 다시 보내지 않는다.
- 새 login credential의 keychain write 뒤 profile index commit과 보상 삭제가 모두 실패하면 `credential_store_state_unknown`을 반환한다. 이때 위의 explicit AuthGate URL/client ID 두 override를 설정한 `auth logout`으로 issuer+client key를 직접 정리한 뒤 다시 로그인한다.
