# Lifecycle

이 문서는 user, agent, space, connection, node, text, file, API key의 생성/삭제 side effect 정본이다.

## User

### Local user 최초 생성

```text
accounts(kind='user')
users
```

- user 생성은 space, agent, API key를 자동 생성하지 않는다.
- 첫 space는 user가 명시적으로 생성한다.
- 재로그인은 PII ciphertext/hash만 갱신하고 space/agent/key를 만들지 않는다.

### User 삭제

User 삭제는 live owned space가 없을 때만 허용한다. Space는 사용자가 먼저 명시적으로 삭제해야 한다.

t=0:

```text
accounts.is_active=false
accounts.deleted_at/deleted_by_account_id 설정
owned agents deactivate
owned user API keys revoke
owned agent API keys revoke
owned agent connections disconnect
```

purge 시점:

```text
users PII ciphertext/hash 제거
provider_sub_hash tombstone 해제
attribution 보존용 account/user shell 유지
```

## Agent

### Agent 생성

User caller만 agent를 생성한다.

```text
accounts(kind='agent')
agents(owner_user_id=user)
```

- API key는 자동 생성하지 않는다.
- Space connection은 자동 생성하지 않는다.
- owner user당 active agent 한도를 넘을 수 없다.

### Agent 삭제

Agent 삭제는 deactivate다.

```text
accounts(kind='agent').is_active=false
agent API keys revoke
space_agent_connections disconnect
```

`agents` row는 attribution 보존을 위해 일반 product action에서 hard delete하지 않는다.

## Space

### Space 생성

User caller만 space를 생성한다.

```text
spaces(owner_user_id=user)
root node '/'
```

- Space는 owner user의 live space 한도를 넘을 수 없다.
- Root node는 생성 transaction의 일부다.
- Agent는 space를 생성할 수 없다.

### Space rename/delete

Owner user만 space 이름 변경/삭제를 수행한다.

삭제는 soft delete다.

```text
spaces.deleted_at=now()
spaces.deleted_by_user_id=caller
spaces.purge_after=now()+retention
```

- 내부 nodes/text/file/connection은 즉시 hard delete하지 않는다.
- Live 조회는 deleted space를 제외한다.
- `purge_after` 이후 background purge가 cascade hard delete할 수 있다.

## Agent connection

Owner user만 agent를 space에 연결/해제한다.

```text
space_agent_connections
  permission = read | write
```

- 연결 대상 agent는 같은 owner user의 active agent여야 한다.
- 연결 대상 space는 caller가 소유한 live space여야 한다.
- `write`는 `read`를 포함한다.
- Connection 변경은 account, agent, space, API key를 만들지 않는다.

## Text and File nodes

### Folder 생성

```text
nodes(kind='folder')
```

### Text 생성/쓰기

```text
nodes(kind='text')
text_objects
```

- Text content는 UTF-8이다.
- `byte_len`, `line_count`, `content_sha256`, `media_type`을 plaintext 기준으로 저장한다.
- 현재 read/write/patch/grep은 plain Text만 대상으로 한다.
- Server-side encrypted Text 저장 컬럼은 DB에 예약되어 있지만 현재 surface에서는 노출하지 않는다.

### File

```text
nodes(kind='file')
file_objects
```

- File schema는 binary/object content를 위해 준비되어 있다.
- 현재 REST/MCP upload/download surface는 노출하지 않는다.
- File은 직접 text read/patch/grep 대상이 아니다.

### Node 삭제

Folder/Text/File 삭제는 soft delete다.

```text
nodes.deleted_at=now()
nodes.deleted_by_account_id=caller
nodes.purge_after=now()+retention
```

Folder recursive delete는 subtree node를 같은 transaction에서 soft delete한다.

## API key

### 생성

User caller만 API key를 만든다.

```text
api_keys(account_id=user_id, created_by_user_id=user_id)       -- user key
api_keys(account_id=agent_id, created_by_user_id=owner_user_id) -- agent key
```

- 평문 token은 생성/rotation 응답에서 한 번만 반환한다.
- User key는 현재 user 자신에게만 만든다.
- Agent key는 caller가 소유한 active agent에게만 만든다.
- `expires_at`은 필수이며 미래 시각이어야 한다.
- User key TTL 최대 30일, agent key TTL 최대 365일.
- User account당 live key 최대 2개, agent account당 live key 최대 5개.

### Revoke/rotation

Revoke:

```text
api_keys.revoked_at=now()
api_keys.revoked_by_user_id=caller
api_keys.revoked_reason=reason
```

Rotation은 같은 account에 새 key를 만들고 old key를 같은 transaction에서 revoke한다. 기존 token 원문은 복구하지 않는다.
