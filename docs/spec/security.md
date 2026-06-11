# Security spec

## 기본 원칙

- Secret, bearer token, OAuth code, PKCE verifier, API key plaintext는 log/error/audit payload에 기록하지 않는다.
- User PII는 평문 저장하지 않는다.
- API key plaintext는 저장하지 않고 HMAC hash만 저장한다.
- Content(Text/File)는 제품 설정이나 space 정책에 따라 server-side encryption으로 저장할 수 있다.
- Node metadata는 content가 아니며 암호화 대상이 아니다.

## Root key domains

```text
ENC_ROOT     PII와 content 암호화용
LOOKUP_ROOT  provider/email/API key lookup HMAC와 session signing용
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
token plaintext = ngk_v1_{key_id}_{secret}
token_hash      = HMAC(API_KEY_SUBKEY, "api-key:v1:" || key_id || ":" || secret)
```

- Plaintext token은 생성/rotation 응답에서 한 번만 반환한다.
- DB에는 `token_hash`, `hash_key_id`, `hash_version`, `token_prefix`만 저장한다.
- LOOKUP root key 폐기가 필요하면 영향받는 live key를 revoke하고 재발급한다.

## Content encryption

Text/File content는 두 저장 방식을 가진다.

```text
plain      = DB 또는 object storage에 평문 content 저장
encrypted  = ENC root에서 파생한 content subkey로 AEAD 암호화 저장
```

Text metadata는 plaintext 기준으로 계산한다.

```text
content_sha256 = SHA256(plaintext bytes)
byte_len       = plaintext byte length
line_count     = plaintext line count for Text
```

암호화 저장 시 DB에는 ciphertext, nonce, enc_key_id, enc_version을 저장한다. REST/MCP content surface는 plain Text만 대상으로 한다. SQL `LIKE/ILIKE` 기반 grep은 encrypted content에 직접 적용하지 않는다.

## Deletion and anonymization

User 탈퇴는 account row를 즉시 hard delete하지 않는다. Attribution 보존을 위해 account shell은 남기고, retention 이후 PII ciphertext/hash와 provider tombstone을 제거한다.

Agent 삭제도 account deactivate로 처리한다. Agent row는 attribution 보존을 위해 일반 product action에서 hard delete하지 않는다.
