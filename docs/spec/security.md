# 보안 정책

이 문서는 notegate의 개인정보(PII) 저장, 암호화, root key 관리, rotation 원칙을 정의한다.
DB 컬럼 구조는 `docs/spec/db.md`를 따르고, 이 문서는 그 컬럼을 어떻게 사용해야 하는지에 대한 정책을 정의한다.

## 기본 원칙

- 사용자 PII 원문은 DB에 평문으로 저장하지 않는다.
- 권한, 조인, 감사에 필요한 식별자는 UUID로 유지한다.
- 사람이 직접 식별될 수 있는 원문은 encrypted ciphertext 또는 HMAC hash로 분리한다.
- API 응답은 권한이 있는 surface에서 필요한 최소 정보만 복호화해 반환한다.
- root secret, 파생 subkey, plaintext PII, API key plaintext는 application log, error message, audit payload에 기록하지 않는다.
- notegate runtime은 active root key 검증과 사용만 담당한다. 무중단 root key rotation은 현재 runtime 목표가 아니다.

## PII 분류

```text
암호화 저장: display_name, email, avatar_url 등 표시/연락용 원문
HMAC 저장: OAuth provider subject, normalized email 등 lookup/unique 비교값
평문 유지: account_id, workspace_id, role, kind, is_active, deleted_at 등 권한/조인 필드
원문 저장 금지: bearer token, OAuth code, PKCE verifier, API key plaintext, provider subject 원문
```

`email_hash`, `provider_sub_hash`는 암호문이 아니더라도 개인정보 보호 대상이다. 접근 권한,
로그 출력, 운영자 조회 범위를 encrypted field와 같은 수준으로 제한한다.

## Root key domain

notegate는 복호화 가능한 데이터와 단순 비교/검증 데이터를 서로 다른 root secret domain으로 분리한다.
Root secret은 DB에 저장하지 않는다.

```text
ENC root
  - PII ciphertext 암호화/복호화용
  - env: NOTEGATE_ENC_ROOT_KEY_ID, NOTEGATE_ENC_ROOT_SECRET

LOOKUP root
  - provider/email lookup HMAC, API key HMAC, session signing용
  - env: NOTEGATE_LOOKUP_ROOT_KEY_ID, NOTEGATE_LOOKUP_ROOT_SECRET
```

일반 runtime은 각 domain의 active root를 하나씩 사용한다.

## 목적별 subkey

Application은 startup 또는 admin command 실행 시 root secret을 읽고 HKDF로 목적별 subkey를 파생한다. Purpose label은 secret이 아닌 코드 상수다.

```text
enc_epoch_verify_subkey    = HKDF(enc_root_secret, "notegate/enc/epoch-verify/v1")
pii_field_subkey           = HKDF(enc_root_secret, "notegate/enc/pii-field/v1")

lookup_epoch_verify_subkey = HKDF(lookup_root_secret, "notegate/lookup/epoch-verify/v1")
provider_sub_hmac_subkey   = HKDF(lookup_root_secret, "notegate/lookup/provider-sub-hmac/v1")
email_hmac_subkey          = HKDF(lookup_root_secret, "notegate/lookup/email-hmac/v1")
api_key_hmac_subkey        = HKDF(lookup_root_secret, "notegate/lookup/api-key-hmac/v1")
session_sign_subkey        = HKDF(lookup_root_secret, "notegate/lookup/session-signing/v1")
```

규칙:

- root secret은 32 bytes 이상의 random secret이어야 한다.
- 같은 raw root secret bytes를 서로 다른 domain에 재사용하지 않는다.
- 같은 raw root secret bytes를 암호화/HMAC/session signing에 직접 사용하지 않는다.
- root secret은 `SecretString`으로 다루고 log/error/audit payload에 노출하지 않는다.
- crypto 연산에는 raw root secret을 직접 쓰지 않고 목적별 subkey만 사용한다.
- DB row에는 key material이 아니라 `crypto_key_epochs.key_id`와 version만 저장한다. `key_id`는 root key epoch를 식별하고, version은 HKDF label/crypto material format version을 뜻한다.
- HKDF label은 코드의 중앙 registry에만 정의한다.
- DB 테이블명, 컬럼명, repository 이름, endpoint 이름을 HKDF label로 자동 변환하지 않는다.
- Table rename, column split/merge 같은 저장소 리팩토링은 기존 ciphertext/hash 호환성을 깨면 안 된다.
- HKDF label을 바꿔야 하면 `/v2`처럼 version을 올리고, 해당 key material로 생성된 row의 version과 migration 절차를 함께 정의한다.
- Label은 secret이 아니다. 목적은 같은 root secret에서 파생된 subkey 간 domain separation이다.

## Key epoch 검증

`crypto_key_epochs`는 env 또는 admin input으로 주입된 root secret이 선언된 `key_id`와 맞는지 검증한다.

```text
verify_tag = HMAC(epoch_verify_subkey, "key-epoch:v1:" + domain + ":" + key_id)
```

규칙:

- startup 시 active ENC root와 active LOOKUP root의 `verify_tag`를 DB와 비교한다.
- `key_id`가 DB에 없거나 `verify_tag`가 다르면 startup 또는 admin command는 실패한다.
- `key_id`는 영구 식별자다. 한 번 등록한 `key_id`는 상태가 바뀌어도 다른 root secret에 재사용하지 않는다.
- `status='active'` root만 새 encrypt/hash/sign에 사용할 수 있다.
- `status='verify_only'` root는 runtime write에는 사용할 수 없다.
- `status='revoked'` root는 사용할 수 없다.

## 암호화 저장 값

PII 원문 암호화는 application layer에서 수행한다.

```text
display_name_ciphertext = AEAD_Encrypt(pii_field_subkey, display_name, display_name AAD)
email_ciphertext        = AEAD_Encrypt(pii_field_subkey, email, email AAD)
```

AAD는 encrypted field의 storage context다. AAD는 secret이 아니며, decrypt 시 재구성 가능한 안정 값만 사용한다.

```text
display_name AAD = app=notegate;field=account.display_name;account_id=<accounts.id>;key_id=<enc_key_id>;version=<enc_version>
email AAD        = app=notegate;field=user.email;account_id=<users.id>;key_id=<enc_key_id>;version=<enc_version>
```

규칙:

- 암호화 알고리즘은 인증 암호화(AEAD)를 사용한다.
- 기본 알고리즘은 `AES-256-GCM`이다.
- 각 encrypted field는 고유 nonce를 사용한다.
- 같은 key와 nonce 조합을 재사용하지 않는다.
- Encrypted row에는 사용한 ENC root `key_id`/version을 함께 저장한다.
- Account별 DEK/envelope table은 현재 두지 않는다.
- AAD에는 plaintext PII, token, OAuth code, PKCE verifier를 넣지 않는다.
- AAD의 `field`는 안정적인 crypto field id다. 물리 table/column 이름을 바꿔도 같은 의미의 데이터면 기존 field id를 유지한다.
- AAD 구성이 달라지면 복호화가 실패하므로 AAD 변경은 version migration으로만 수행한다.

## HMAC 조회 값

OAuth provider subject와 email lookup 값은 원문 대신 HMAC hash로 저장한다.

```text
provider_sub_hash = HMAC(provider_sub_hmac_subkey, "provider-sub:v1:" + provider + ":" + provider_subject)
email_hash        = HMAC(email_hmac_subkey, "email:v1:" + normalize(email))
```

규칙:

- HMAC key material은 LOOKUP root secret에서 파생한다.
- Hash row에는 사용한 LOOKUP root `key_id`/version을 함께 저장한다.
- `email_hash`는 email ciphertext를 복호화할 수 있으므로 admin 작업에서 재계산할 수 있다.
- `provider_sub_hash`는 provider subject 원문을 저장하지 않으므로 일괄 재계산할 수 없다.

## API key 저장과 rotation

API key는 비밀번호와 같은 credential로 취급한다. API key plaintext는 DB에 저장하지 않고, 암호화해 복호화 가능하게 저장하지도 않는다.

```text
plaintext token = ngk_v1_<api_key_id>_<secret>
token_hash     = HMAC(api_key_hmac_subkey, "api-key:v1:" + api_key_id + ":" + secret)
```

규칙:

- 평문 token은 생성 또는 rotation 응답에서 정확히 한 번만 반환한다.
- DB에는 `token_prefix`, `token_hash`, `hash_key_id`, `hash_version`만 저장한다.
- `hash_key_id`는 token_hash를 만든 LOOKUP root key id다.
- API key 자체 rotation은 new token 발급 + old key revoke로 처리한다. Token 원문은 복구하거나 복호화하지 않는다.
- LOOKUP root key를 폐기해야 할 때 기존 API key는 원문이 없어 일괄 rehash할 수 없다. 영향받는 `hash_key_id`의 live API key는 revoke하고, 사용자 또는 agent creator가 새 key를 생성하도록 요구한다.

## 탈퇴와 익명화

User 탈퇴나 agent 삭제는 account hard delete가 아니라 deactivate/soft delete로 처리한다.
`created_by`, `updated_by`, `deleted_by` 참조를 보존하기 위해 account row는 남긴다.

탈퇴 lifecycle side effect는 `docs/spec/lifecycle.md`의 User 탈퇴 정책을 따른다. 이 문서는 PII ciphertext/hash 제거와 key material 비노출 정책을 정본으로 둔다.

## 로그와 감사 payload

감사로그 상세 스키마는 별도 spec에서 정의한다. 다만 모든 로그와 감사 payload는 다음 원칙을 지켜야 한다.

- plaintext PII를 기록하지 않는다.
- bearer token, OAuth code, PKCE verifier, API key plaintext를 기록하지 않는다.
- root secret, subkey, key material을 기록하지 않는다.
- 필요한 경우 account_id, workspace_id, event kind, result, timestamp 중심으로 기록한다.
