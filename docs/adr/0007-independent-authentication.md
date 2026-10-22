# ADR 0007: 외부 OIDC와 서비스 자체 OAuth의 책임 분리

## 상태와 목표

제안. 이 ADR은 독립 서비스 전환의 목표 계약이며 현재 런타임의 동작은
[`REST 인증`](../spec/rest/auth.md), [`MCP 인증`](../spec/mcp/auth.md),
[`CLI`](../spec/cli.md)를 따른다. 각 계약은 후속 구현 PR에서 해당 명세와 함께 반영한다.

브라우저·MCP·CLI가 AuthGate 없이 동작하고 표준 OIDC 제공자를 교체할 수 있게 한다.
기존 `account_id`, 문서·버전·이력·소유권과 암호화 데이터는 보존한다. 새 이름은 미정이다.
서비스가 제공하는 OAuth는 이 제품의 리소스 접근에 한정한다.

## 책임과 의존 방향

```text
Google / 설정된 다른 OIDC 제공자
                  │ 신원 확인
                  v
api: OIDC adapter / browser·OAuth HTTP endpoints
                  │
                  v
service: identity 연결 / session·grant·token lifecycle
                  │
                  v
db: 기존 accounts·users / external identities / OAuth 상태

검증된 credential -> Caller(account_id) -> 기존 command·권한·문서 서비스
```

- 외부 OIDC는 로그인 시 신원을 확인한다. AuthGate도 같은 adapter로 선택할 수 있지만 필수 서비스가 아니다.
- 브라우저에는 로컬 opaque session을, MCP·CLI에는 자체 OAuth access token을 발급한다.
- 외부 ID token·access token을 제품의 MCP·CLI bearer로 허용하지 않는다.
- 기존 Agent API key와 채널별 credential 경계, `user_mcp_enabled`, Space connection 권한을 유지한다.
- HTTP envelope와 외부 통신은 api에, 상태 전이와 권한 규칙은 service에, 원자성과 제약은 db에 둔다.
  문서·검색·버전 서비스는 OIDC 제공자와 토큰 형식을 알지 않는다.
- 기존 crate 안에서 모듈을 나눈다. 새 store trait나 공용 인증 제품용 crate를 선제적으로 만들지 않는다.

## 외부 신원과 기존 계정

`external_identities`는 하나의 local user에 여러 외부 신원을 연결한다. 신원은 검증된 issuer와
subject의 조합으로 식별하고 DB에는 issuer와 subject HMAC 및 key/version을 저장한다.
동일 신원이 여러 계정에 연결되지 않도록 DB UNIQUE로 강제한다. Issuer는 discovery와 ID token의
검증된 값으로 정확히 비교하며 이메일과 표시 이름은 연결 키로 사용하지 않는다.

기존 `users.provider_sub_hash`는 `authgate` namespace로 계산되어 있다. 이 해시만으로 Google의
subject나 새 namespace의 해시를 계산할 수 없다. 이전은 다음 규칙을 따른다.

1. 기존 AuthGate issuer를 이전 설정에 명시하고 DB의 이전 상태에 고정한다. 새 로그인 제공자에서 추측하지 않는다.
2. 그 issuer로 검증된 로그인만 기존 해시를 조회할 수 있다. 기존 UUID를 유지하며 새 identity를 같은 transaction에 연결한다.
3. Google 연결은 기존 계정에 대한 최근 인증과 새 OIDC 신원 검증을 모두 요구한다. Callback을 계정·flow·issuer에 묶고 CSRF를 검증한다.
   별도 관리 이전은 검증된 일대일 매핑만 허용한다. 같은 이메일이라는 이유로 자동 연결하거나 새 계정으로 우회하지 않는다.
4. 재시도는 같은 결과를 반환한다. 다른 계정이 이미 소유한 identity 연결과 동시 연결 충돌은 거부한다.

제공자 전환 중에는 계정 연결과 명시적인 신규 가입을 구분한다. 알 수 없는 신원으로 로그인했다는
이유만으로 새 계정을 자동 생성하지 않는다. 기존 신원을 제거할 때는 마지막 로그인 수단을 잃지 않게 한다.

[ADR 0004](0004-account-deletion-and-re-registration-policy.md)의 삭제·재가입 정책을 함께 적용한다.
삭제와 identity 연결은 같은 account row를 잠가 직렬화한다. 비활성 계정은 연결·로그인·grant 발급을
거부하고 identity tombstone은 기존 계정 보존 기간 동안 유지한다. 익명화 때 모든 연결 신원과
legacy 해시를 함께 제거하되 attribution용 account/user shell은 보존한다.

## OIDC 로그인과 브라우저 세션

- 운영자가 설정한 issuer에서 discovery한다. Endpoint를 경로 문자열로 조합하지 않는다.
  HTTPS와 issuer 검증을 유지하고 신뢰한 metadata가 가리키는 다른 origin의 endpoint도 지원한다.
  사용자 입력 URL이나 token header가 조회 목적지를 선택하지 못하게 하며 HTTP redirect를 따르지 않는다.
- Authorization Code + PKCE S256, state·nonce, issuer·audience·만료·서명과 UserInfo subject 검증을 유지한다.
  Public client와 confidential client의 인증 방식은 등록과 discovery에 맞춰 설정한다.
- 필요한 scope는 `openid profile email`이다. 제공자별 추가 파라미터는 허용 목록으로 관리하며
  client ID·redirect URI·state·nonce·PKCE 같은 보안 파라미터를 덮어쓰지 못하게 한다.
- 외부 토큰은 신원 확인에만 사용한다. Google API 접근과 외부 refresh token의 장기 저장은 이 계약에 포함하지 않는다.
  기존 암호화된 AuthGate refresh token은 이전 세션 종료까지 legacy 경로에서만 취급한다.
- 로그인 흐름은 서버의 만료되는 flow record로 관리하고 브라우저와 issuer에 묶는다.
  병렬 브라우저 로그인·MCP 승인·계정 연결이 서로의 state나 PKCE verifier를 덮어쓰지 않아야 한다.
- 로컬 세션에는 인증 identity와 인증 시각을 연결한다. Identity 연결·해제와 OAuth 승인은 최근 인증을 요구한다.
  일반 API는 외부 IdP에 접속하지 않고 세션과 현재 local account 상태를 검증한다.
- 브라우저 logout은 해당 로컬 세션을 폐기한다. 전체 로그아웃과 연결 앱 철회는 별도 명시적 동작이다.
  제공자 설정 변경 시 이전 credential을 새 제공자로 보내지 않는다.

로컬 세션 정책은 외부 refresh token과 분리한다. 아래 제안에서는 최대 30일 세션 안에서도
24시간마다 OIDC 재인증을 요구한다. 외부 계정 정지·로그아웃의 즉시 동기화는 보장하지 않는다.
민감 동작에는 5분 이내의 검증된 인증을 요구하며, 재인증 freshness는 제공자별로 실제 검증한다.
지원하는 `max_age`·`auth_time` 계약을 확인하고 token의 `iat`를 사용자 인증 시각으로 대신하지 않는다.

## 자체 OAuth 계약

자체 issuer는 제품의 공개 URL에 고정한다. 제품은 OIDC Provider가 아닌 OAuth authorization server로
동작하며 자체 ID token이나 UserInfo API를 발급·제공할 필요는 없다.

| 경계 | 계약 |
|---|---|
| MCP | Authorization Code + 필수 PKCE S256, 정확한 redirect URI, 사용자 승인 |
| CLI | RFC 8628 Device Flow, 사용자 승인, polling 제한과 `slow_down` 처리 |
| Discovery | RFC 8414 AS metadata와 RFC 9728 protected-resource metadata를 실제 지원 기능에서 생성 |
| Client 등록 | 공식 CLI는 사전 등록 public client. MCP는 사전 등록과 CIMD 지원. DCR은 실제 대상 클라이언트가 요구할 때 추가 |
| Client 인증 | Public client는 `none`. 사전 등록 confidential client는 `client_secret_basic`과 저장된 secret 검증. 실제 지원 방식만 광고 |
| Resource | MCP와 CLI는 각각의 canonical resource에 바인딩. CLI의 기존 client ID audience는 새 resource 계약으로 전환 |
| Scope | 제품 진입 권한 `mcp`·`cli`와 선택적 `offline_access`. 실제 작업 권한은 기존 service 규칙과 교집합으로 판정 |
| Access token | 256-bit 이상 CSPRNG opaque credential, DB에는 용도를 분리한 HMAC만 저장 |
| Refresh token | Client·resource·scope·grant에 바인딩한 opaque credential, HMAC 저장, 매 사용 시 원자적으로 회전 |

제품이 발급한 client secret도 무작위 값과 용도를 분리한 HMAC으로 관리한다. 외부 OIDC client secret은
배포 Secret에서 주입하며 source·일반 설정 출력·이력에 포함하지 않는다.

Token 형식을 클라이언트가 해석하지 않게 한다. Opaque token은 signing key·JWKS 배포를 추가하지 않고
폐기 상태를 DB에서 확인할 수 있어 단일 제품에 맞는다. 공용 DB의 indexed lookup과 grant·account
상태 확인을 함께 수행하며 권한 폐기를 지연시키는 process-local 인증 캐시는 도입하지 않는다.
DB 부하와 지연은 기존 JWT 경로 대비 CI에서 측정한 뒤 운영 전환한다.

OAuth 승인은 client 이름·식별자, resource와 허용 범위를 보여준다. 승인 POST는 브라우저 세션에
바인딩하고 CSRF를 검증한다. 승인 전후의 권한을 다시 확인하며 재승인 없는 scope 확대는 거부한다.
CIMD 조회는 HTTPS·공인 목적지 제한, DNS 재검증, redirect 금지, timeout·응답 크기·cache 상한을
적용하고 client ID와 metadata의 일치를 검증한다. Loopback redirect 예외는 native client 계약에만 둔다.

인가 코드의 client·resource·redirect URI·PKCE 검증과 단일 소비, grant 생성, token 저장은 같은
DB transaction에서 처리한다. Device code도 pending/approved/denied/consumed 상태와 만료를
구분하고 승인된 요청의 token 발급은 한 번만 허용한다. User code 추측과 과도한 polling을 제한한다.

Refresh는 family row를 잠가 검증·소비·새 credential 저장을 함께 commit한다. 소비된 refresh token의
재사용은 family와 연결 access token을 폐기한다. CLI는 기존 profile별 잠금과 응답 유실 시
재로그인하는 정책을 유지한다. 네트워크 오류에 이전 refresh token을 무조건 재전송하지 않는다.

Grant 철회·전체 로그아웃·계정 삭제 이후 새 요청은 폐기된 credential을 거부한다. 계정 삭제는
기존 session·Agent credential 정리와 함께 OAuth grant도 폐기한다. 이미 시작한 요청과 발급된
presigned URL은 기존 완료·만료 계약을 따른다. 만료·폐기 판정은 동기 경로에서 하고 background
purge는 dead row 회수만 담당한다. 관측용 `last_used_at`은 만료·권한 판정에 사용하지 않는다.
신원 연결·grant 승인·폐기는 기존 audit/security 기록과 보존 정책에 연결한다. Credential 원문,
OAuth code·PKCE verifier·외부 token을 log·Changes·호출 이력에 저장하지 않는다.

### 초기 수명 정책 제안

| 항목 | 수명 |
|---|---|
| 로그인 flow | 5분 |
| Authorization code | 60초, 한 번 소비 |
| Device code | 5분, polling 기본 간격 5초 |
| Access token | 10분 |
| Refresh family | 마지막 성공 회전부터 7일, 최초 승인부터 최대 30일 |
| Browser session | 최초 발급부터 최대 30일, OIDC 재인증 주기 24시간 |
| 민감 동작의 인증 freshness | 5분 |

재인증과 token 회전은 최초 absolute expiry를 연장하지 않는다. 시각은 주입 가능한 clock으로
전달하고 `now >= expires_at`이면 만료로 처리한다. 실제 Postgres transaction의 동시성도 검증한다.
이 값은 현재 런타임 설정이 아닌 초기 출시 정책 제안이다.

## 라이브러리 재사용 평가

2026년 10월 17일 기준 공개 문서·배포 metadata와 source의 평가다. Compile·보안 감사·적합성 검증을 대신하지 않는다.

| 후보 | 확인된 역할 | 결정 |
|---|---|---|
| 기존 `openidconnect` 4.0.1 | OIDC client와 protocol message type. 완전한 provider의 credential·session 관리는 제공하지 않음 | 기존 외부 로그인 adapter에 재사용 |
| `oxide-auth` 0.6.1 / `oxide-auth-async` 0.2.1 | OAuth server primitive와 async adapter, 필수 S256 PKCE 구성 가능 | Code flow 재사용 후보. 아래 적합성 검증 전 의존성 추가·채택 확정 안 함 |
| `oxide-auth-axum` 0.6.0 | 배포된 manifest가 Axum 0.8과 `oxide-auth` 0.6을 사용 | Framework major는 일치. 실제 async/SQLx 조합은 CI 검증 필요 |
| Rauthy | Rust 기반 독립 IdP/SSO 제품 | 외부 제품 운영 의존을 옮기는 선택이므로 이번 내장 인가 모듈에는 도입하지 않음 |
| AuthGate와 zitadel/oidc | 기존 Go 기반 OAuth 동작과 token lifecycle의 참고 구현 | 계약·회귀 사례 참고. 서비스나 Go runtime 의존을 추가하지 않음 |

OAuth server 후보를 채택하려면 Axum·Tokio·SQLx 조합에서 다음을 CI로 입증해야 한다.
단일 code 소비, 필수 S256과 redirect/resource 검증, PostgreSQL refresh 회전·재사용 탐지,
DB commit 이후 HTTP 응답 유실, RFC 오류 응답을 유지해야 한다. Device Flow·CIMD·metadata도
요구 계약과의 차이를 확인한다. 메모리 저장 예제의 존재를 production 지원으로 간주하지 않는다.
검증하지 못한 후보를 제거할 때는 의존성과 실험 코드도 함께 제거한다.

## 이전과 PR 순서

PR은 앞선 PR의 검증·병합 후 하나씩 올린다. 각 구현에서 현재 spec을 갱신한다.

1. **Identity 기반**: `external_identities`, 검증된 issuer 전달, legacy issuer에 한정한 해시 호환,
   기존 UUID 보존, 삭제·tombstone·purge 대응과 실제 Postgres 회귀 테스트. 로그인 제공자와 토큰 발급자는 유지한다.
2. **OIDC와 계정 연결**: 범용 provider adapter, 두 신원을 검증하는 연결 UX, local session 전환,
   Google/AuthGate profile 계약 테스트. 양쪽을 검증하지 못한 기존 계정은 미이전 상태로 남기고 자동 신규 계정으로 대체하지 않는다.
3. **OAuth 기반**: server library의 CI 적합성 검증을 먼저 수행하고 채택 결과를 기록한다.
   Grant·client·code·token transaction, metadata, 사용자 승인·철회 UI를 구현한다.
4. **MCP와 CLI**: 자체 issuer의 Code/Device Flow, client 등록, scope/resource 검증과 credential 저장·갱신·폐기를 통합한다.
   기존 CLI의 hard-coded scope와 discovery 확장을 바꾸고 구·신 CLI/server 조합의 호환 구간과 업그레이드 순서를 검증한다.
5. **전환 검증**: 실제 client 호환 검증, AuthGate 통신 차단 검증, 기존 DB 이전 연습, 배포·롤백 절차를 검증한다.
6. **이름과 출시**: 새 이름이 정해지면 CLI keychain·profile·다운로드 경로와 배포 참조를 이전한다.
   도메인 변경이 있다면 issuer 전환과 같은 운영 전환으로 묶어 token 재발급을 반복하지 않는다.

Schema는 additive로 도입하고 legacy 경로는 전환 전까지 유지한다. 이전 snapshot·호환 구간·rollback
가능한 binary/schema 조합을 검증한 뒤 legacy 해시와 설정을 제거한다. 새 issuer가 발급한 credential을
이전 binary가 처리할 수 없는 시점부터는 단순 binary rollback을 금지한다. 호환 binary로 복귀하거나
신규 credential 폐기·재로그인을 포함한 절차로 복구하며 검증되지 않은 DB snapshot 복원을 자동 실행하지 않는다.

이름 변경에도 기존 암호화 HKDF label·AAD·HMAC namespace와 API key 해석을 보존한다.
형식 변경이 필요하면 별도 version과 이전 reader를 둔다. 표시 이름의 일괄 치환으로 영속 형식을 바꾸지 않는다.

## 첫 구현 PR과 출시 검증

첫 구현은 위 1번으로 제한한다. `model`의 verified identity, `service/identity`, `db` identity repo·migration,
account 삭제·purge, api의 verified issuer 전달과 관련 테스트를 수정한다. OIDC client 교체, 자체 token
발급, 제품 이름과 기존 문서 서비스는 이 PR에서 바꾸지 않는다.

- 같은 subject라도 issuer가 다르면 충돌하지 않고 legacy issuer만 기존 account를 조회한다.
- 동일 신원 동시 로그인·연결은 하나의 결과로 수렴하고 다른 account에 이중 연결되지 않는다.
- 삭제 중·삭제된 account는 연결·재로그인으로 부활하지 않는다. 익명화 후에는 기존 재가입 정책을 따른다.
- 이전 전후 document·version·Changes·Agent의 owner/actor UUID와 기존 ciphertext 복호화가 유지된다.

출시 전에는 실제 대상 MCP client와 공식 CLI의 로그인·승인·권한 변경·철회·재접속을 검증한다.
AuthGate 통신이 차단된 환경에서 브라우저·MCP·CLI가 동작해야 한다. Google 실제 로그인 검증은
등록된 OAuth client를 사용한 별도 검증 환경에서 수행하며 CI에는 production credential을 제공하지 않는다.
기존 제한된 100 VU 성능 시나리오와 인증 경로 지표를 비교하되 이를 1,000명 동시 사용자 보장으로 해석하지 않는다.
코드·DB 테스트, build와 lint는 기존 CI에서 실행한다.

## 근거

- [MCP Authorization 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- [OAuth Security BCP RFC 9700](https://www.rfc-editor.org/rfc/rfc9700.html)
- [Device Authorization RFC 8628](https://www.rfc-editor.org/rfc/rfc8628.html)
- [OAuth Access Token RFC 6749](https://www.rfc-editor.org/rfc/rfc6749.html#section-1.4)
- [Google OIDC](https://developers.google.com/identity/openid-connect/reference)
- [openidconnect](https://docs.rs/openidconnect/4.0.1/openidconnect/)
- [oxide-auth](https://docs.rs/oxide-auth/0.6.1/oxide_auth/),
  [PKCE source](https://github.com/197g/oxide-auth/blob/0e3ef86d924aa5546dc3275d1d7269ea63edfaf1/oxide-auth/src/code_grant/extensions/pkce.rs),
  [배포된 crate 의존성](https://crates.io/api/v1/crates/oxide-auth-axum/0.6.0/dependencies)
- [Rauthy](https://github.com/sebadob/rauthy)
