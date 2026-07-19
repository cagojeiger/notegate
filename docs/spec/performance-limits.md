# Performance and hard limits

Limit은 두 층이다.

1. **System hard max**: 코드와 DB가 절대 넘기지 않는 제품 상한.
2. **User tier quota**: user별 실제 사용량 상한. 항상 system hard max 이하이다.

현재 tier는 `tier0`와 `system_max`만 둔다. 신규 user의 기본 tier는 `NOTEGATE_DEFAULT_USER_TIER`로 정하며 값은 `tier0` 또는 `system_max`다. 설정하지 않으면 `system_max`를 사용한다. 운영 배포 전에는 이 값을 `tier0`로 설정한다.

Quota에 포함되는 live usage의 의미, counter 갱신, 분산 reconciliation과 전체 재계산 정책은 `usage-and-quotas.md`를 따른다.

## HTTP safety limits

```text
request_body_max_bytes = 2097152      # 2 MiB JSON/body 기본 상한
server_timeout_seconds = 30
control_plane_timeout_seconds = 5      # /health, /ready 등 control-plane 요청
rate_limit_global_per_process = 600/minute
```

`/health`, `/ready`는 rate limit 대상에서 제외한다.

## Account and credential limits

System hard max:

```text
spaces_per_user_max = 20 live spaces per user
agents_per_user_max = 50 active agents per user
connections_per_space_max = 50 active agent connections per space
connected_spaces_per_agent_max = 100 live spaces per agent
user_api_keys_per_account_max = 2 live user keys
agent_api_keys_per_account_max = 5 live agent keys
user_api_key_max_ttl_days = 30
agent_api_key_max_ttl_days = 365
api_key_name_max_chars = 63
agent_name_max_chars = 63
oauth_provider_sub_max_chars = 255
user_display_name_max_chars = 128
user_email_max_chars = 254
space_name_max_chars = 63
space_name_forbidden = '/', ':', control chars, leading/trailing whitespace, '.', '..'
node_name_max_chars = 128
node_name_forbidden = '/', control chars, leading/trailing whitespace, '.', '..'
```


User tier quotas:

```text
tier0
  spaces_per_user = 1
  agents_per_user = 3
  connections_per_space = 5
  connected_spaces_per_agent = 5
  space_max_text_bytes = 134217728       # 128 MiB live Text content per space
  space_max_file_bytes = 134217728       # 128 MiB live File content per space
  space_max_nodes = 2000
  folder_max_children = 200

system_max
  spaces_per_user = 20
  agents_per_user = 50
  connections_per_space = 50
  connected_spaces_per_agent = 100
  space_max_text_bytes = 1073741824      # 1 GiB
  space_max_file_bytes = 1073741824      # 1 GiB
  space_max_nodes = 25000
  folder_max_children = 1000
```

API key 개수와 TTL, 요청 크기, node/content 단위 제한은 security/product safety limit으로 tier별로 나누지 않는다.

## Tree and content limits

아래 값은 `system_max`와 동일한 system hard max다. 실제 사용량은 user tier quota가 더 낮을 수 있다.

```text
Space-level limits
  space_max_text_bytes = 1073741824      # 1 GiB live Text content per space
  space_max_file_bytes = 1073741824      # 1 GiB live File content per space
  space_max_nodes = 25000                # live folder/text/file nodes per space
  max_tree_depth = 7                     # segments below root
  max_path_bytes = 903                   # derived path byte upper bound
  folder_max_children = 1000             # live direct children per folder

Node/content-level limits
  node_name_max_chars = 128
  text_max_bytes = 1048576               # 1 MiB per text object
  text_max_lines = 5000                  # lines per text object
  file_max_bytes = 104857600             # 100 MiB per file product hard max
  node_metadata_max_bytes = 16384        # 16 KiB per node metadata object
  node_metadata_max_depth = 4
  node_metadata_key_max_chars = 64
  node_metadata_string_max_chars = 2048
```

`space_max_text_bytes`와 `space_max_file_bytes`는 독립 quota다. Soft-deleted node의 bytes는 live quota에 포함하지 않는다. S3 object bytes는 soft delete transaction에서 비동기 삭제 대상으로 전환한다.

S3 호환 object upload는 single PUT으로 `file_max_bytes`까지 지원한다.

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

Node/content-level
  text bytes exceeded:
    400 invalid_input "text exceeds the maximum of {max} bytes; split the text into smaller notes"
  text lines exceeded:
    400 invalid_input "text exceeds the maximum of {max} lines; split the text into smaller notes"
  file bytes exceeded:
    400 invalid_input "file exceeds the maximum of {max} bytes"
  metadata bytes/depth/key/string exceeded:
    400 invalid_input with the exceeded metadata limit
```

## Pagination defaults

```text
spaces_default_limit = 50
spaces_max_limit = 100
agents_default_limit = 100
agents_max_limit = 100
connections_default_limit = 100
connections_max_limit = 100
children_default_limit = 100
children_max_limit = 200
nodes_default_limit = 50
nodes_max_limit = 100
read_default_max_lines = 200
read_max_lines = 5000
read_default_max_bytes = 65536          # 64 KiB
read_max_bytes = 1048576                # 1 MiB
find_default_limit = 50
grep_default_limit = 20
search_max_limit = 100
search_query_max_chars = 256
search_children_page_max = 200
search_candidate_page_max = 1000
search_node_scan_max = 1000            # node summaries inspected per request
grep_scan_budget_bytes = 8388608       # 8 MiB content bytes per request
search_glob_patterns_max = 32          # include/exclude list length per request
search_glob_pattern_max_chars = 256    # one include/exclude glob pattern
search_response_target_bytes = 262144  # 256 KiB response target
api_keys_default_limit = 50
api_keys_max_limit = 100
```

목록 API는 여러 row를 반환하면 opaque cursor pagination을 제공한다. 내부 구현은 resource hard limit에 따라 DB keyset 또는 bounded in-memory pagination을 사용할 수 있다.

## Search memory model

Search는 MCP/CLI command이며 REST resource API에는 노출하지 않는다. Search는 folder scope의 subtree를 DFS pre-order로 순회한다. 내부 구조는 DB candidate scan과 application matcher의 2단계다.

최악의 논리 scan 범위:

```text
node scan upper bound       = 25000 live nodes per space
plain text scan upper bound = 1 GiB live Text content per system_max space
                          = 128 MiB live Text content per tier0 space
```

최악의 경우 전체 subtree를 탐색해야 하지만 한 요청에서 전체를 읽지 않는다. `limit`은 반환할 result 수이고 scan budget은 검사할 candidate 양이다. Scan budget에 먼저 도달하면 result가 없어도 `has_more=true`와 `next_cursor`를 반환할 수 있다.

```text
DB candidate inspect <= 1000 node summaries per request
node scan budget    <= 1000 node summaries per request
grep scan budget    <= 8 MiB content bytes per request
grep text read      <= 8 MiB total content bytes per request
include glob list   <= 32 patterns × 256 chars
exclude glob list   <= 32 patterns × 256 chars
result limit        <= 100 node summaries
response target     <= 256 KiB
```

DB candidate scan은 DFS order를 `sort_order, name, id`와 내부 `sort_path`로 안정화한다. Cursor는 마지막으로 소비한 candidate의 위치를 기억하고, 다음 page는 그 이후 candidate부터 검사한다. Regex matching은 DB regex가 아니라 application Rust regex로 수행한다.

`grep`은 기본적으로 query를 포함하는 Text node 후보를 반환한다. 요청 옵션으로 matching line number를 반환할 수 있지만 본문과 snippet은 별도 read command로 조회한다.

## Storage sizing guideline

1만 user 기준 `system_max` hard-limit worst case는 다음과 같다. `tier0`만 사용하면 content worst case는 user당 256 MiB다.

```text
spaces_per_user_max = 20
space_max_text_bytes = 1 GiB
space_max_file_bytes = 1 GiB
worst_case_logical_content_per_user = 40 GiB
worst_case_logical_content_10000_users = 400 TiB

tier0_logical_content_per_user = 256 MiB
tier0_logical_content_10000_users = 2.5 TiB
```

운영 sizing은 평균 사용률을 별도로 가정한다.

```text
10000 users, 2% usage -> logical 8 TiB, PostgreSQL physical 12~16 TiB estimate
10000 users, 5% usage -> logical 20 TiB, PostgreSQL physical 30~40 TiB estimate
```

PostgreSQL physical estimate는 row overhead, index, TOAST, WAL, dead tuple, vacuum 여유, backup 여유를 포함해 logical content의 약 1.5~2배로 본다.

1만 user 규모에서는 PgBouncer와 connection pool 상한을 전제로 한다. File content가 hard-limit worst case에 가까워지면 object storage와 backup/restore 전략을 별도로 둔다.

## Purge limits

```text
deleted_space_retention_days = 30
deleted_node_retention_days = 30
account_deletion_retention_days = 15
subtree_delete_max_nodes = 1000
api_key_retention_days = 30
purge_batch_spaces = 100
purge_batch_nodes = 1000
purge_batch_accounts = 100
purge_batch_api_keys = 1000
```
