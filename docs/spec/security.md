# Security spec

## 기본 원칙

- Secret, bearer token, OAuth code, PKCE verifier, API key plaintext, browser session token, OAuth refresh token은 log/error/audit payload에 기록하지 않는다.
- MCP invocation history는 제품 개선과 실패 분석을 위해 서버에 도달한 `tools/call`의 redacted 입력·응답 snapshot을 JSONB로 저장한다. 저장 전 tool/op별 allowlist와 redaction policy를 적용하며 Text 본문/edit/diff와 grep 일치 줄, 검색어와 cursor, 원본 파일명과 암호화 metadata, presigned URL/header, ETag, PII, protocol `_meta`, 알 수 없는 field 값은 redaction marker 또는 omission count로 대체한다. MCP wire `content` 복사본은 저장하지 않고 redacted `structured_content`만 사용한다. Owner user의 browser self-review에만 노출하고 90일 후 bounded purge한다.
- User PII는 평문 저장하지 않는다.
- API key plaintext는 저장하지 않고 HMAC hash만 저장한다.
- Browser session cookie token은 저장하지 않고 HMAC hash만 저장한다.
- OAuth refresh token은 browser client에 노출하지 않고 서버에서 암호화 저장한다.
- Text content는 서버가 읽는 plain content 또는 client-side encrypted payload로 저장한다. `system_max` Space는 plain content를 서버 관리 방식으로 추가 암호화할 수 있다.
- Node metadata는 content가 아니며 암호화 대상이 아니다.
- Markdown frontmatter는 Text content 안의 YAML block이다. encrypted Text 안에 있으면 content와 함께 client-side encrypted payload에 포함된다.

## Root key domains

```text
ENC_ROOT     PII, browser refresh token, 서버 관리 Text 암호화용
LOOKUP_ROOT  provider/email/API key/browser session lookup HMAC와 session signing용
```

각 root key는 `crypto_key_epochs`에 `key_id`, `domain`, `status`, `verify_tag`, `version`으로 등록한다. 빈 DB에서는 프로세스 시작 시 active epoch row를 생성한다. 이미 active epoch가 존재하면 환경 변수의 active root key와 DB registry가 맞지 않을 때 서버는 시작하지 않는다.

## PII storage

```text
users.provider_sub_hash = HMAC(LOOKUP_SUBKEY, "provider-sub:v1:" || provider || ":" || sub)
users.email_hash        = HMAC(LOOKUP_SUBKEY, "email:v1:" || normalized_email)
users.email_ciphertext  = AEAD_ENCRYPT(ENC_SUBKEY, email, aad)
accounts.display_name_ciphertext = AEAD_ENCRYPT(ENC_SUBKEY, display_name, aad)
```

Agent name은 제품 메타데이터로 평문 저장한다. Agent name에 사람 PII를 넣지 않는 것은 제품 입력 정책으로 다룬다.

## API key storage

```text
token plaintext = ngk_v2_{key_id}_{secret}
token_hash      = HMAC(API_KEY_SUBKEY, "api-key:v1:" || key_id || ":" || secret)
```

- Plaintext token은 생성/rotation 응답에서 한 번만 반환한다.
- DB에는 `token_hash`, `hash_key_id`, `hash_version`, `token_prefix`만 저장한다.
- Token format version과 HMAC domain version은 별도 계약이다. Token은 `ngk_v2_`만 발급하고 해석하며 HMAC domain은 v1을 유지한다.
- API key는 Agent account만 소유할 수 있으며 DB trigger가 User-owned key와 v2가 아닌 key의 신규 저장을 거부한다.
- 인증 시 token에서 계산한 `token_prefix`와 DB 값을 정확히 비교한다.
- LOOKUP root key 폐기가 필요하면 영향받는 live key를 revoke하고 재발급한다.

## Browser session storage

```text
session plaintext = ngs_v1_{session_id}_{secret}
session_hash      = HMAC(SESSION_TOKEN_SUBKEY, "browser-session:v1:" || session_id || ":" || secret)
refresh_token     = AEAD_ENCRYPT(ENC_SUBKEY, authgate_refresh_token, aad)
```

- Browser session token plaintext는 HttpOnly cookie에만 들어간다.
- DB에는 `token_hash`, `hash_key_id`, `hash_version`, `token_prefix`만 저장한다.
- AuthGate refresh token은 `browser_sessions.refresh_token_*` 컬럼에 암호화 저장한다.
- Refresh token은 AuthGate token endpoint에 제출할 때만 복호화한다.
- Refresh 응답에 새 refresh token이 있으면 기존 encrypted refresh token을 교체한다.
- Refresh 응답의 subject가 기존 user와 다르면 local browser session을 revoke하고 401로 처리한다.
- FE는 refresh token과 browser session token 원문을 JavaScript storage에 저장하지 않는다.

## Text content encryption

Text content의 API 저장 형식과 DB at-rest 암호화 상태는 별도 값이다.

```text
storage_format='plain'     = 서버가 읽을 수 있는 UTF-8 content
storage_format='encrypted' = client-side encrypted payload

at_rest_encryption='none'   = plain content를 content_text에 저장
at_rest_encryption='server' = plain content를 서버가 AEAD 암호화해 저장
```

Client-side encrypted Text에서 서버는 원문, 비밀번호, 복호화 키를 받거나 저장하지 않는다. 서버는 encrypted payload를 opaque JSON object로 저장하고 반환한다. Encrypted payload metric은 서버의 canonical JSON serialization 기준으로 계산한다. Canonical JSON은 UTF-8, object key 정렬, 불필요한 whitespace 없음, 동일 JSON value의 동일 byte serialization을 의미한다.

서버 관리 암호화는 `storage_format='plain'`에만 적용한다. Ciphertext는 Space id, Node id, key id, version을 AEAD AAD로 묶는다. API read, write, patch와 `grep`은 서버에서 복호화한 plain content를 사용한다. Node metadata, `content_sha256`, `byte_len`, `line_count`는 암호화하지 않는다.

`grep`은 복호화된 plain body를 process-local memory에 제한적으로 캐시할 수 있다. Cache key는 Space/Node/content SHA를 묶고, 기본 capacity는 plaintext byte weight 128 MiB, TTL은 30분, TTI는 5분이다. Cache entry는 외부 저장소나 다른 replica로 전송하지 않으며 process 종료, capacity eviction, TTL/TTI 만료 시 참조에서 제거된다. Rust allocator가 해제된 memory page를 즉시 zeroize하거나 OS에 반환한다는 보장은 없다.

`text_objects.at_rest_encryption`은 서버 관리 암호화의 실제 저장 상태다. 설정을 변경하면 기존 plain Text 본문을 같은 transaction에서 즉시 암호화하거나 복호화한다. Space 기본값은 새 Text 생성 시 초기 저장 상태를 정하며 기존 Text를 바꾸지 않는다.

서버 관리 암호화 설정 변경은 Space owner User만 할 수 있다. Agent는 write 권한이 있어도 활성화하거나 비활성화할 수 없다. 암호화 활성화와 암호화 저장은 Space owner의 tier capability `text_encryption`이 필요하다. 현재 `system_max`만 허용한다. Tier가 낮아져도 기존 ciphertext는 읽기와 검색이 가능하지만 새 암호화 저장은 거부한다. 서버는 tier 변경을 이유로 ciphertext를 자동 복호화 저장하지 않는다.

```text
plain content_sha256 = SHA256(plaintext bytes)
plain byte_len       = plaintext byte length
plain line_count     = plaintext line count

encrypted content_sha256 = SHA256(canonical encrypted payload JSON bytes)
encrypted byte_len       = canonical encrypted payload JSON byte length
encrypted line_count     = 0
```

REST는 client-side encrypted payload 저장/조회가 가능하다. MCP Text content operation과 `search op=grep`은 `storage_format='plain'`만 대상으로 하며, 서버 관리 암호화 여부는 이 동작을 바꾸지 않는다.

## Node write lock

Node write lock은 content 암호화와 독립된 구조 변경 방지 정책이다. 직접 잠금은 `nodes.write_locked`에만 저장하고 descendant의 상속 상태는 materialize하지 않는다. 따라서 잠금 source와 실제 tree 관계가 항상 일치한다.

직접 잠금 변경은 Browser channel의 Space owner User만 할 수 있다. Agent와 MCP/API channel의 User는 write permission이 있어도 변경할 수 없다. `write_lock` tier capability가 새 잠금 설정을 제어하며 현재는 `system_max`만 활성화한다. Tier 하향 뒤에는 기존 잠금을 해제할 수 있다.

Node mutation은 현재 node의 ancestor chain을 확인한다. Folder rename/move/delete는 subtree에 직접 잠긴 descendant가 있는지도 확인한다. Read와 File download, owner의 Space 삭제는 별도 권한 경계이며 허용한다.

File upload handle은 등록 transaction에서 destination의 write lock을 확인한다. 이후 잠금은 이미 등록된 handle의 완료를 취소하지 않으며, 완료 시 일반 write permission과 File 생성 invariant는 다시 확인한다.


## File content encryption

File content는 S3 호환 object storage에 저장하며 두 encryption mode를 가진다.

```text
none    = 서버가 저장 bytes를 그대로 반환
client  = client-side encrypted bytes
```

`encryption_mode=client`에서 서버는 원본, 비밀번호, 복호화 키를 받거나 저장하지 않는다. `byte_len`은 저장된 bytes 기준이다. File의 `content_sha256`은 저장하거나 노출하지 않는다.

## Object storage access

NoteGate object storage credential은 설정된 bucket의 `objects/*`에 대한 `GetObject`, `PutObject`, `DeleteObject`만 허용한다. Bucket 생성, bucket 목록 조회, 익명 접근과 관리 작업은 허용하지 않는다. MinIO root credential은 로컬 초기화에만 사용하며 NoteGate runtime에 전달하지 않는다.

## Deletion and anonymization

User 탈퇴는 account row를 즉시 hard delete하지 않는다. Attribution 보존을 위해 account shell은 남기고, retention 이후 PII ciphertext/hash와 provider tombstone을 제거한다.

Agent 삭제도 account deactivate로 처리한다. Agent row는 attribution 보존을 위해 일반 product action에서 hard delete하지 않는다.
