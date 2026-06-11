# Domain model

이 문서는 notegate의 제품 용어 정본이다.

## Core concepts

```text
Account    인증 가능한 실행 주체. kind는 user 또는 agent다.
User       사람 계정. Space와 Agent를 소유하고 관리한다.
Agent      User가 만든 worker 계정. API key로 인증되고 연결된 Space에서 작업한다.
Space      User가 소유한 중앙 저장 범위.
Node       Space 안 tree item. folder/text/file 중 하나이며 metadata를 가진다.
Folder     하위 node를 담는 container.
Text       plain UTF-8 또는 client-side encrypted payload content object.
File       binary/object content. 직접 text read/patch/grep 대상이 아니다.
Connection Agent와 Space 사이의 연결. permission은 read 또는 write다.
API key    User 또는 Agent account로 인증되는 bearer credential.
Metadata   Node에 붙는 JSON object. content가 아니며 서버가 읽을 수 있다.
```

## Ownership and control

```text
User owns Spaces
User owns Agents
User creates API keys
User connects Agents to Spaces
Agent acts only inside connected Spaces
```

`accounts`는 attribution과 authentication을 위한 공통 actor다. `users`와 `agents`는 account subtype이다.

## Permission model

```text
User caller:
  owned space read/write/manage
  owned agent manage
  own user API key manage
  owned agent API key manage

Agent caller:
  connected space read/write according to permission
  no space management
  no agent/key management
```

Permission:

```text
read  = list/stat/read text/read file/read metadata/find/grep
write = read + create folder/create text/upload file/update/append/patch/delete/move/write metadata
```

## Naming rules

- 외부 제품 용어는 `space`, `agent connection`, `permission`, `text`, `file`을 사용한다.
- 일반 authorization 설명에는 `access`라는 보안 용어를 쓸 수 있지만, 제품 리소스 이름으로는 `connection`을 사용한다.
- 권한 용어는 `read`, `write`, `permission`을 사용한다.
