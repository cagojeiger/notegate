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
file_inline_pg_max_bytes = 262144       # 256 KiB 이하 file은 PG inline 저장 가능
file_max_bytes = 104857600             # 100 MiB per uploaded file
```

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
api_keys_default_limit = 50
api_keys_max_limit = 100
```

목록 API는 여러 row를 반환하면 pagination을 제공한다.

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
