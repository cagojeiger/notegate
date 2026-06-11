# Search

검색은 Space 단위 권한을 먼저 확인한 뒤 실행한다.

## Authorization

```text
user caller:
  space.owner_user_id = caller_user_id

agent caller:
  active connection exists
  permission read 또는 write
```

## find

`find`는 node metadata 검색이다.

대상:

```text
nodes.kind IN ('folder','text','file')
nodes.name
```

Root node `/`는 결과에서 제외한다.

## grep

`grep`은 plain Text content 검색이다.

대상:

```text
nodes.kind = 'text'
text_objects.content_text
```

- File은 grep 대상이 아니다.
- Encrypted Text는 현재 grep 대상이 아니다.
- 초기 구현은 Postgres `ILIKE`/trigram 기반 단순 검색으로 유지한다.
- 결과는 keyset pagination을 제공한다.

## Folder scope

특정 folder 하위 검색은 subtree node id 목록을 제한한 뒤 검색한다. Tree depth와 children hard limit이 있으므로 scope expansion은 bounded operation이다.
