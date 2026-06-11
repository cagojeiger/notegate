# MCP Files

MCP files tools는 Space tree를 path-first로 다룬다.

## Common

```text
files_ls
files_stat
files_mkdir
files_touch_text
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
files_read_text
files_write_text
files_patch_text
```

- Text node에만 적용한다.
- Content는 UTF-8이다.
- Patch는 exact-match이며 각 old 문자열은 정확히 한 번만 매칭되어야 한다.

예:

```json
{"target":"personal:/memory/state.json"}
```

```json
{"target":"personal:/memory/state.json","content":"{\"ok\":true}\n"}
```

## File

```text
files_upload
files_download
```

- File node에만 적용한다.
- 256 KiB 이하 file은 PG inline 저장 가능하다.
- 더 큰 file은 object storage 구현에서 다룬다.
- File은 `files_read_text`/`files_patch_text`/`grep_text` 대상이 아니다.
