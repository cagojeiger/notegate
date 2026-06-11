# Performance and hard limits

모든 limit은 시스템 hard limit이다. Runtime effective quota는 이 값을 넘지 않는다.

## HTTP safety limits

```text
request_body_max_bytes = 2097152      # 2 MiB JSON/body 기본 상한
server_timeout_seconds = 30
rate_limit_global_per_process = 600/minute
```

`/health`, `/ready`는 rate limit 대상에서 제외한다.

## Account and credential limits

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
space_name_max_chars = 63
space_name_pattern = ^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$
```

## Tree and content limits

```text
node_name_max_chars = 128
max_tree_depth = 5
folder_max_children = 200
space_max_nodes = 10000 live nodes
space_max_texts = 5000 live text nodes
space_max_files = 5000 live file nodes
space_max_text_bytes = 268435456       # 256 MiB live text total per space
text_max_bytes = 1048576               # 1 MiB per text object
file_inline_pg_max_bytes = 262144       # 현재 저장 가능한 file bytes 상한
file_max_bytes = 104857600             # file product hard max
node_metadata_max_bytes = 16384         # 16 KiB per node metadata object
node_metadata_max_depth = 4
node_metadata_key_max_chars = 64
node_metadata_string_max_chars = 2048
```

현재 File 저장 구현은 `file_inline_pg_max_bytes` 이하만 지원한다. `file_max_bytes`는 file 객체의 제품 hard max이며, 현재 지원 크기를 의미하지 않는다.

Depth는 root 아래 segment 수로 계산한다.

```text
/                 depth 0
/notes            depth 1
/notes/today      depth 2
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
find_default_limit = 50
grep_default_limit = 20
search_max_limit = 100
search_children_page_max = 200
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

Search는 MCP/CLI command이며 REST resource API에는 노출하지 않는다. Search는 folder scope의 subtree를 DFS pre-order로 순회한다.

최악의 논리 scan 범위:

```text
node scan upper bound       = 10000 live nodes per space
plain text scan upper bound = 256 MiB live text bytes per space
```

최악의 경우 전체 subtree를 탐색해야 하지만 한 요청에서 전체를 읽지 않는다. `limit`은 반환할 result 수이고 scan budget은 검사할 candidate 양이다. Scan budget에 먼저 도달하면 result가 없어도 `has_more=true`와 `next_cursor`를 반환할 수 있다.

```text
children page        <= 200 node summaries
node scan budget     <= 1000 node summaries
grep scan budget     <= 8 MiB content bytes
grep text read batch <= grep_scan_budget_bytes / text_max_bytes
                      현재 hard limit 기준 최대 8 text objects
include glob list    <= 32 patterns × 256 chars
exclude glob list    <= 32 patterns × 256 chars
result limit         <= 100 node summaries
response target      <= 256 KiB
```

`grep`은 match line이 아니라 query를 포함하는 Text node 후보를 반환한다. 본문은 별도 read command로 조회한다.

## Purge limits

```text
deleted_space_retention_days = 30
deleted_node_retention_days = 30
account_deletion_retention_days = 15
api_key_retention_days = 30
purge_batch_spaces = 100
purge_batch_nodes = 1000
purge_batch_accounts = 100
purge_batch_api_keys = 1000
```
