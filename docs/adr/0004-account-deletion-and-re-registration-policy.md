# ADR 0004: Account deletion and re-registration policy

## Context

notegate는 user PII를 다루며, user가 소유한 spaces와 agents를 함께 관리한다. Account id는 Text/File 변경 attribution에도 사용되므로 즉시 hard delete하면 과거 작업 기록이 깨진다.

## Decision

User 삭제는 deactivate 후 retention 경과 시 PII를 익명화한다. Product resource는 user 소유 모델에 맞춰 자동 정리한다.

삭제 시작 시점:

```text
user account deactivate
owned spaces soft delete
owned agents deactivate
owned user API keys revoke
owned agent API keys revoke
owned agent connections disconnect
```

Retention 경과 후:

```text
users PII ciphertext/hash 제거
provider_sub_hash tombstone 해제
account/user shell 유지
```

같은 provider subject의 재가입은 tombstone retention 동안 거부한다. Tombstone이 해제된 뒤에는 새 local user 생성 흐름을 탄다.

## Consequences

- Attribution row는 유지된다.
- 삭제된 user는 재활성화하지 않는다.
- 개인용 product model이므로 공동 owner/팀 멤버 정리 정책은 두지 않는다.
- Space hard delete는 purge job이 처리한다.
