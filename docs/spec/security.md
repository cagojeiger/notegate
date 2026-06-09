# 보안 정책

이 문서는 notegate의 개인정보(PII) 저장, 암호화, 키 관리, 삭제 처리 원칙을 정의한다.
DB 컬럼 구조는 `docs/spec/db.md`를 따르고, 이 문서는 그 컬럼을 어떻게 사용해야 하는지에
대한 정책을 정의한다.

## 기본 원칙

- 사용자 PII 원문은 DB에 평문으로 저장하지 않는다.
- 권한, 조인, 감사에 필요한 식별자는 UUID로 유지한다.
- 사람이 직접 식별될 수 있는 원문은 encrypted ciphertext 또는 HMAC hash로 분리한다.
- API 응답은 권한이 있는 surface에서 필요한 최소 정보만 복호화해 반환한다.
- key material, pepper, plaintext PII는 application log, error message, audit payload에 기록하지 않는다.

## PII 분류

```text
암호화 저장: display_name, email, avatar_url 등 표시/연락용 원문
HMAC 저장: OAuth provider subject, normalized email 등 lookup/unique 비교값
평문 유지: account_id, workspace_id, role, kind, is_active, deleted_at 등 권한/조인 필드
원문 저장 금지: bearer token, OAuth code, PKCE verifier, API key plaintext, provider subject 원문
```

`email_hash`, `provider_sub_hash`는 암호문이 아니더라도 개인정보 보호 대상이다. 접근 권한,
로그 출력, 운영자 조회 범위를 encrypted field와 같은 수준으로 제한한다.

## 암호화 방식

- PII 원문 암호화는 application layer에서 수행한다.
- DB는 ciphertext, nonce, hash, key wrapping metadata만 저장한다.
- 암호화 알고리즘은 인증 암호화(AEAD)를 사용한다.
- 기본 알고리즘은 `AES-256-GCM`이다.
- 각 encrypted field는 고유 nonce를 사용한다.
- 같은 key와 nonce 조합을 재사용하지 않는다.

## HMAC 조회 값

OAuth provider subject와 email lookup 값은 원문 대신 HMAC hash로 저장한다.

```text
provider_sub_hash = HMAC(pepper, provider + ":" + provider_subject)
email_hash        = HMAC(pepper, normalize(email))
```

규칙:

- pepper는 DB 밖에서 관리한다.
- hash에는 version을 함께 저장한다.
- pepper rotation 중에는 old/new pepper를 모두 조회할 수 있어야 한다.
- 로그인 또는 profile update 성공 시 최신 pepper version으로 hash를 갱신한다.
- 충분한 migration 기간 뒤 old pepper를 폐기한다.

## API key 저장과 hash secret rotation

API key는 비밀번호와 같은 credential로 취급한다. API key plaintext는 DB에 저장하지 않고, DEK로 암호화해 복호화 가능하게 저장하지도 않는다.

```text
plaintext token = ngk_v1_<key_id>_<secret>
token_hash     = HMAC(api_key_hash_secret, key_id + ":" + secret)
```

규칙:

- 평문 token은 생성 또는 rotation 응답에서 정확히 한 번만 반환한다.
- DB에는 `token_prefix`, `token_hash`, `hash_key_id`, `hash_version`만 저장한다.
- `api_key_hash_secret` material은 DB 밖에서 관리한다.
- `api_key_hash_keys`는 secret의 id/version/status metadata만 저장한다.
- API key 자체 rotation은 new token 발급 + old key revoke로 처리한다. Token 원문은 복구하거나 복호화하지 않는다.

Hash secret status:

```text
current      -> 새 key 발급과 rehash에 사용
verify_only  -> 정상 rotation 중 해당 hash key로 저장된 API key 검증에만 사용
compromised  -> 유출 의심. 해당 hash_key_id의 live API key는 인증 거부/revoke 대상
retired      -> 더 이상 검증에 사용하지 않음
```

정상 hash secret rotation:

1. 새 `current` hash secret을 등록한다.
2. 교체 대상 hash secret은 `verify_only`로 둔다.
3. `verify_only` key로 검증 성공한 API key는 요청에 포함된 token plaintext를 이용해 current key로 opportunistic rehash할 수 있다.
4. live DB와 backup retention에서 verify-only key 참조가 사라지면 `retired`로 전환하고 secret material을 폐기한다.

Hash secret compromise 처리:

- hash secret 유출이 의심되면 해당 `hash_key_id`를 `compromised`로 표시한다.
- `compromised` hash key로 저장된 live API key는 투명 rehash하지 않고 revoke한다.
- 사용자는 새 API key를 발급받아야 한다.
- app runtime compromise가 의심되면 API key뿐 아니라 browser session secret, OAuth client secret, PII KEK 접근 가능성도 함께 조사한다.

## DEK/KEK 구조

Account별 data encryption key(DEK)를 사용한다. DEK는 key encryption key(KEK)로
wrap해서 `account_encryption_keys.wrapped_dek`에 저장한다.

Application은 설정 또는 KMS/HSM adapter로 주입되는 KEK를 사용한다. DB 스키마와 repository 계약은 KEK 제공 방식과 독립적이다.

```text
KEK
  └─ account DEK
       └─ account/user PII ciphertext
```

규칙:

- KEK material은 application DB에 저장하지 않는다.
- application은 unwrap 결과로 얻은 DEK를 필요한 작업 범위 안에서만 사용한다.
- account DEK는 account 단위 PII를 암호화하는 용도로만 사용한다.
- document content 암호화가 필요해지면 별도 key domain으로 분리한다.

## KEK 회전

KEK rotation은 새 암호화와 rewrap에 최신 KEK를 사용하게 하는 방식으로 처리한다.
PII ciphertext 전체를 즉시 재암호화하지 않는다.

```text
old KEK version -> decrypt-only
new KEK version -> encrypt/decrypt active
```

운영 방식:

- 새 PII 암호화나 DEK wrap은 최신 KEK를 사용한다.
- `wrapped_dek`는 read/update 시 lazy rewrap한다.
- 오래된 KEK version을 참조하는 row는 background rewrap job으로 점진 갱신할 수 있다.
- decrypt-only KEK version은 live DB와 backup retention에서 참조가 사라질 때까지 보관한다.
- decrypt-only KEK version 폐기는 참조 row와 복구 가능한 backup 범위를 확인한 뒤 수행한다.

## DEK 회전

DEK rotation은 PII ciphertext 재암호화가 필요하므로 기본 정기 작업으로 보지 않는다.
다음 상황에서 수행한다.

```text
DEK 유출 의심
암호화 정책 변경
강제 migration
보안 사고 대응
```

DEK rotation 절차:

```text
old DEK로 decrypt
new DEK 생성
new DEK로 PII 재암호화
new DEK를 최신 KEK로 wrap
account_encryption_keys 갱신
```

## 탈퇴와 익명화

User 탈퇴나 agent 삭제는 account hard delete가 아니라 deactivate/soft delete로 처리한다.
`created_by`, `updated_by`, `deleted_by` 참조를 보존하기 위해 account row는 남긴다.

탈퇴 lifecycle side effect는 `docs/spec/lifecycle.md`의 User 탈퇴 정책을 따른다. 이 문서는 PII 제거와 crypto shredding의 보안 정책을 정본으로 둔다.

더 강한 삭제가 필요한 경우 account DEK를 `destroyed_at` 처리해 crypto shredding한다. 이 경우
남아 있는 ciphertext는 복호화할 수 없다.

## 로그와 감사 payload

감사로그 상세 스키마는 별도 spec에서 정의한다. 다만 모든 로그와 감사 payload는 다음 원칙을
지켜야 한다.

- plaintext PII를 기록하지 않는다.
- bearer token, OAuth code, PKCE verifier, API key plaintext를 기록하지 않는다.
- key material, DEK, pepper를 기록하지 않는다.
- 필요한 경우 account_id, workspace_id, event kind, result, timestamp 중심으로 기록한다.
