# Performance and hard limits

Limit은 두 층이다.

1. **System hard max**: 코드와 DB가 절대 넘기지 않는 제품 상한.
2. **User tier quota**: user별 실제 사용량 상한. 항상 system hard max 이하이다.

현재 tier는 `tier0`와 `system_max`만 둔다. 신규 user의 기본 tier는 `NOTEGATE_DEFAULT_USER_TIER`로 정하며 값은 `tier0` 또는 `system_max`다. 설정하지 않으면 `tier0`를 사용한다.

Quota에 포함되는 live usage의 의미, counter 갱신, 분산 reconciliation과 전체 재계산 정책은 `usage-and-quotas.md`를 따른다.

## HTTP safety limits

HTTP 상한과 기본 bucket은 [limits](../../backend/crates/core/src/limits.rs), 환경 설정과 검증은 [Config](../../backend/crates/core/src/config.rs)를 따른다.

Ingress limit은 auth, metadata, OpenAPI, `/api/v1`, `/api/v2`, `/mcp`, `/mcp/v2`를 함께 보호한다.
Browser V1, Public V2, User MCP, Agent MCP V2에는 독립적인 process-local bucket을 추가로 적용한다. 한
surface의 요청이 다른 surface의 전용 bucket을 소모하지 않지만 ingress 상한은 공유한다.
각 surface의 RPS와 burst는 ingress 값을 넘을 수 없다. 이 값은 account/tier quota가 아니라
process 보호를 위한 safety limit이다.

기본값은 다음 환경 변수로 pod마다 독립 조정한다.

```text
NOTEGATE_HTTP_RATE_LIMITS__INGRESS__REQUESTS_PER_SECOND
NOTEGATE_HTTP_RATE_LIMITS__INGRESS__BURST
NOTEGATE_HTTP_RATE_LIMITS__BROWSER_V1__REQUESTS_PER_SECOND
NOTEGATE_HTTP_RATE_LIMITS__BROWSER_V1__BURST
NOTEGATE_HTTP_RATE_LIMITS__PUBLIC_V2__REQUESTS_PER_SECOND
NOTEGATE_HTTP_RATE_LIMITS__PUBLIC_V2__BURST
NOTEGATE_HTTP_RATE_LIMITS__MCP__REQUESTS_PER_SECOND
NOTEGATE_HTTP_RATE_LIMITS__MCP__BURST
NOTEGATE_HTTP_RATE_LIMITS__MCP_V2__REQUESTS_PER_SECOND
NOTEGATE_HTTP_RATE_LIMITS__MCP_V2__BURST
```

replica가 여러 개면 cluster 전체 허용량은 pod별 상한의 합으로 증가한다. Cluster-wide rate
limit은 ingress/gateway에서 별도로 적용한다.

검색의 in-flight 상한 초과는 대기시키지 않고 거부한다. `/health`, `/ready`는 rate limit 대상에서 제외한다.

## Best-effort metadata write-behind

API key와 browser session의 `last_used_at`, preview에서 감지한 File media type은 요청 경로에서
직접 UPDATE하지 않는다. Pod별 process-local buffer의 대기 map은 종류별 최대 10000개의 서로
다른 key를 보관하고 30초마다 PostgreSQL bulk UPDATE로 비운다. Flush 중에는 분리된 batch와 새
대기 map이 함께 존재할 수 있어 종류별 최대 20000개 항목이 일시적으로 메모리에 있을 수 있다. 같은 credential은 key 하나로 합치고,
같은 File은 가장 먼저 관측한 감지값으로 합친다. API key와 browser session의 사용 시각은 pod
시계가 아니라 PostgreSQL `now()`로 기록하며, 마지막 저장 후 1시간이 지난 row만 실제로 갱신한다.

상한에 도달하면 이미 들어 있는 key의 값은 계속 합치되 새로운 key는 버리고 구조화 warning을
남긴다. DB flush 실패나 5초 timeout 시 실패 batch를 먼저 보존해 재시도하고, graceful shutdown에서는 HTTP
요청이 모두 끝난 뒤 최대 3회 최종 flush한다. 종료 후에도 남은 항목은 종류별 개수를 error log와
metric으로 남긴다. 이 값들은 인증이나 content의 source of truth가
아닌 best-effort 파생 metadata이므로 pod 강제 종료 시 마지막 flush interval의 관측값은 유실될
수 있다. Replica 간 buffer 공유나 전달은 하지 않는다.

## Account and credential limits

Account·credential·이름 상한은 [limits](../../backend/crates/core/src/limits.rs)를 따른다.

Tier별 quota는 [UserTier::quota](../../backend/crates/core/src/tier.rs)를 따른다.

Tier별 capability는 [UserTier::features](../../backend/crates/core/src/tier.rs)를 따른다.

`text_encryption`과 `write_lock`은 Space owner tier에서 계산한다. Agent caller의 tier는 연결된 Space의 기능을 바꾸지 않는다.

API key 개수와 TTL, 요청 크기, node/content 단위 제한은 security/product safety limit으로 tier별로 나누지 않는다.

## Tree and content limits

Tree·content의 system hard max는 `system_max` quota와 같다. 실제 사용량은 user tier quota가 더 낮을 수 있다.

Tree·content·metadata 상한은 [limits와 Limits](../../backend/crates/core/src/limits.rs)를 따른다.

`space_max_text_bytes`와 `space_max_file_bytes`는 독립 quota다. 휴지통 bytes도 quota에 포함한다. Text는 DB 삭제 commit, File은 S3 삭제 acknowledgement를 기록하는 transaction에서 반환한다. Soft delete는 30일 보관하며 영구 삭제 요청 또는 만료 후 비동기 정리를 시작한다. 집계 기준은 `usage-and-quotas.md`를 따른다.

Upload의 제품·브라우저 상한은 [limits](../../backend/crates/core/src/limits.rs), multipart와 URL batch는 [upload flow](../../backend/crates/api/src/object_upload_flow.rs)와 [object storage](../../backend/crates/api/src/object_storage.rs), 브라우저 동시성은 [UploadProvider](../../frontend/web/src/features/uploads/UploadProvider.tsx)를 따른다.

Multipart 호환 검증 대상은 MinIO다. S3 provider는 incomplete multipart 자동 abort 정책을 제공해야 한다.

Depth는 root 아래 segment 수로 계산한다.

```text
/                 depth 0
/notes            depth 1
/notes/today      depth 2
```

## Limit error contract

Limit 초과는 다음 분류와 메시지를 사용한다. 메시지는 사용자가 다음 행동을 판단할 수 있어야 한다.

```text
Account/tier-level
  owned spaces exceeded:
    409 conflict "owner already has the maximum of {max} spaces for tier {tier}"
  active agents exceeded:
    409 conflict "creator already has the maximum of {max} active agents for tier {tier}"
  agent connections exceeded:
    409 conflict "space already has the maximum of {max} active agent connections for tier {tier}"
  connected spaces exceeded:
    409 conflict "agent is already connected to the maximum of {max} spaces for tier {tier}"

Space-level
  nodes exceeded:
    409 conflict "space already has the maximum of {max} live nodes"
  Text content bytes exceeded:
    409 conflict "space Text content would exceed the maximum of {max} bytes; delete or split Text items"
  file bytes exceeded:
    409 conflict "space files would exceed the maximum of {max} bytes; delete files"
  folder children exceeded:
    409 conflict "folder already has the maximum of {max} live children; split into subfolders"
  depth exceeded:
    400 invalid_input "path is too deep"
  path bytes exceeded:
    400 invalid_input "path is too long"

Node/content-level
  text bytes exceeded:
    400 invalid_input "text exceeds the maximum of {max} bytes; split the text into smaller notes"
  text lines exceeded:
    400 invalid_input "text exceeds the maximum of {max} lines; split the text into smaller notes"
  file bytes exceeded:
    400 invalid_input "file exceeds the maximum size of {max} bytes"
  metadata bytes/depth/key/string exceeded:
    400 invalid_input with the exceeded metadata limit
```

## Pagination defaults

목록·본문·검색의 기본값과 상한은 [limits](../../backend/crates/core/src/limits.rs)를 따른다.

목록 API는 여러 row를 반환하면 opaque cursor pagination을 제공한다. 내부 구현은 resource hard limit에 따라 DB keyset 또는 bounded in-memory pagination을 사용할 수 있다.

## Search limits

숫자 제한의 정본은 위에 연결한 코드다. DFS 순회, cursor, scan 중단, `has_more`, Rust matcher와 복호화 본문 cache의 동작은 [`search.md`](./search.md)가 정본이다.

## Purge limits

주기와 전체 timeout은 [purge 등록](../../backend/crates/api/src/reconciliations/purge.rs), batch와 DB timeout은 [purge repository](../../backend/crates/db/src/purge_repo), 보존 기간은 [limits](../../backend/crates/core/src/limits.rs)를 따른다.
