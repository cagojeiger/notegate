# MCP Files

MCP files tools는 Space tree를 path-first로 다룬다.

## Common

```text
files_ls
files_stat
files_mkdir
files_touch
files_mv
files_rm
```

예:

```json
{"target":"personal:/notes"}
```

`target`은 `space:/path` 축약형이다.

## Text

```text
files_read
files_write
files_patch
```

- Text node에만 적용한다.
- Content는 plain UTF-8이다.
- Encrypted Text content는 MCP files tools에서 읽거나 쓸 수 없다.
- Patch는 exact-match이며 각 `old_text` 문자열은 정확히 한 번만 매칭되어야 한다.

예:

```json
{"target":"personal:/memory/state.json"}
```

```json
{"target":"personal:/memory/state.json","content":"{\"ok\":true}\n"}
```

## File

MCP upload/download tool은 제공하지 않는다. File은 `files_read`/`files_patch`/`files_grep` 대상이 아니다.
