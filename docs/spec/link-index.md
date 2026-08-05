# 내부 링크 인덱스

NoteGate는 같은 Space 안의 Markdown 링크와 이미지 참조를 현재 상태 투영으로 관리한다. 이 인덱스는 문서 본문의 정본이 아니며, `docs/spec/markdown-links.md`의 path 해석 규칙을 그대로 따른다.

## 범위

인덱싱 대상은 표준 Markdown의 일반 링크와 이미지 참조다.

```text
[문서](../guide.md)
![이미지](./assets/diagram.png)
```

외부 URL, 현재 문서 anchor, Obsidian wikilink, raw HTML 링크는 저장하지 않는다. Query string이 있거나 path가 유효하지 않은 내부 후보는 invalid 참조로 저장한다. Client-encrypted Text는 서버가 본문을 읽을 수 없으므로 인덱싱하지 않는다. 서버 저장 암호화 Text는 복호화한 현재 본문을 인덱싱하며 `raw_href`, 정규화 경로, source/target 관계는 허용된 평문 metadata로 저장한다.

검색 포함 여부와 링크 인덱싱 여부는 서로 독립적이다.

## 데이터 모델

`node_link_refs`의 한 row는 source 문서 안의 하나의 고유한 Markdown 참조를 나타낸다. 같은 source에서 같은 종류와 `raw_href`가 반복되면 한 row에 `occurrence_count`로 합친다.

```text
node_link_refs
  space_id
  source_node_id
  target_node_id              nullable
  reference_kind             link | image
  raw_href
  normalized_target_path     nullable
  occurrence_count
```

- outgoing은 `source_node_id`로 조회한다.
- incoming은 `target_node_id`로 조회한다.
- 별도 backlink row를 만들지 않는다.
- `target_node_id`가 없어도 원문의 `raw_href`와 정규화 가능한 path를 보존한다.
- 참조 대상은 반드시 같은 Space 안에서만 resolve한다.

상태는 저장된 boolean 하나로 중복하지 않고 조회 시 계산한다.

```text
resolved = target_node_id가 있고 target node가 live
deleted  = target_node_id가 있으나 target node가 soft-deleted
missing  = normalized_target_path가 있으나 live target이 없음
invalid  = 내부 path 후보이지만 정규화할 수 없음
```

UI는 `deleted`, `missing`, `invalid`를 모두 Broken으로 집계하되 상세 원인은 유지한다.

## 갱신 계약

파일 변경 transaction은 `file_change_events`를 기록하면서 같은 transaction에서 해당 Space의 `desired_generation`를 전진시킨다. 이벤트의 `link_index_generation`은 Space 상태 row를 잠근 상태에서 부여한다. 같은 Space의 다음 transaction은 앞선 transaction이 commit 또는 rollback될 때까지 기다리므로, 전역 event id의 발급 순서와 관계없이 commit된 변경 순서대로 연속된 generation을 갖는다. 서로 다른 Space는 서로의 상태 row를 잠그지 않는다.

따라서 변경은 성공했지만 링크 인덱싱 작업이 사라지는 상태를 만들지 않는다. 마이그레이션 전에 존재한 Space는 `uninitialized`로 시작하며 사용자가 Space Inspector에서 최초 인덱싱을 요청해야 한다. 마이그레이션 전에 쌓인 event에는 generation을 소급 부여하지 않고, 최초 전체 인덱싱으로 현재 상태를 구성한다.

새로 생성한 Space는 빈 graph가 이미 현재 상태이므로 `ready`로 시작한다. 이후 변경은 증분 처리한다. `uninitialized` Space에서도 변경 generation은 계속 전진하지만 worker는 최초 인덱싱 요청 전까지 이를 claim하지 않는다. 주기적인 전체 Space scan은 수행하지 않는다.

Worker는 Space별 상태 row를 lease로 claim한다. 여러 API pod가 동시에 실행되어도 하나의 Space는 한 worker만 처리한다. 서로 다른 Space는 병렬 처리할 수 있다.

새 parser version의 worker는 이전 version으로 구성된 Space를 재인덱싱한다. 구버전 worker는 이미 더 높은 parser version으로 구성된 Space를 claim하지 않으므로 rolling deployment 중 투영을 이전 형식으로 되돌리지 않는다. 더 높은 parser version이 DB에 있으면 구버전 pod는 readiness를 통과하지 못한다. Parser version 배포는 forward-only이며 복구는 새 version으로 roll forward한다.

일반 Text 변경은 이벤트 payload의 중간 내용을 재생하지 않는다.

```text
여러 Text 변경 event
  -> source_node_id 중복 제거
  -> 현재 저장된 최종 본문 읽기
  -> 현재 outgoing 참조 전체 파싱
  -> source의 기존 참조 삭제
  -> bounded chunk로 새 결과 기록
  -> 모든 chunk 완료 후 applied_generation 전진
```

따라서 같은 문서가 연속으로 여러 번 저장되어도 worker가 처리할 때 읽은 마지막 현재 상태가 투영된다. 참조 삭제와 각 insert chunk는 claim lease를 검증하고 연장하는 짧은 transaction으로 나눈다. 중간에는 일부 참조만 보일 수 있지만 모든 chunk가 성공하기 전에는 `applied_generation`을 전진시키지 않는다. 실패한 작업은 같은 source의 참조를 다시 삭제한 뒤 최신 본문으로 덮어쓰므로 재시도 후 수렴한다.

Node `revision`은 동기 mutation 충돌을 막는 per-node token이고, `desired_generation`/`applied_generation`은 비동기 Space projection의 진행 상태다. Link worker는 revision event를 재생하지 않고 generation 범위에서 중복 제거한 source의 최신 본문을 읽으므로 두 값은 서로 대체하지 않는다.

Move, rename, recursive copy처럼 여러 source의 상대 path 또는 여러 target path를 바꾸는 topology 변경은 부분 영향을 추측하지 않고 Space 재인덱싱으로 승격한다. 일반 변경에서 source 작업 상한을 넘으면 연속 generation prefix를 상한만큼 증분 처리하고 나머지는 다음 claim에서 이어서 처리한다. 참조를 생략하거나 전체 Space 재인덱싱으로 승격하지 않는다. 삭제된 source의 outgoing 참조는 제거한다. 삭제된 target을 가리키는 참조는 삭제하지 않고 `deleted`로 보존한다.

## 최종 일관성

Space별 상태는 다음 cursor를 가진다.

```text
desired_generation = 정본 변경이 도달한 위치
applied_generation = 링크 투영이 반영한 위치
```

- `applied_generation < desired_generation`이면 결과는 갱신 중일 수 있다.
- 두 cursor가 같고 상태가 ready이면 같은 event 위치까지 반영되었다.
- `uninitialized`는 기존 Space의 최초 전체 인덱싱이 아직 요청되지 않은 상태다. 이 상태에서는 관계 결과를 노출하지 않는다.
- event retention gap, parser version 변경, 안전하게 범위를 계산할 수 없는 topology 변경은 전체 재인덱싱을 요구한다.
- 실패한 작업은 backoff 후 재시도한다. 사용자가 실패한 재인덱싱을 다시 요청하면 저장된 cursor를 유지한 채 즉시 재개한다.
- 전체 재인덱싱은 기존 graph를 한 번에 지우지 않고 bounded source batch마다 현재 결과로 덮어쓴 뒤 cursor를 commit하고 claim을 반환한다. Lease가 batch 도중 만료되면 이후 chunk와 cursor commit은 거부되고 다른 worker가 마지막 commit cursor부터 Space를 다시 claim한다.
- source 재작성 도중 실패하면 부분 결과가 남을 수 있다. 다만 cursor는 전진하지 않으며 다음 재시도가 같은 source를 먼저 지우고 최신 결과를 다시 기록하므로 source 재작성과 Space 재인덱싱은 결과적으로 멱등이다.

재인덱싱은 문서 read/write를 막지 않는다. Node Inspector는 `uninitialized`, `rebuilding`, `failed` 상태에서 부분 관계를 정상 결과처럼 노출하지 않는다. 실패한 재구성을 사용자가 다시 요청해도 완료 전까지 `rebuilding`을 유지한다.

## 삭제와 재생성

- source soft delete: 해당 source의 outgoing 참조를 제거한다.
- target soft delete: incoming 참조를 유지하고 `deleted`로 표시한다.
- target hard purge: `target_node_id`만 null로 전환하고 보존된 path로 `missing`을 유지한다.
- 같은 path에 새 node 생성: unresolved 또는 deleted 참조를 새 node id에 다시 bind한다.
- 링크 존재 여부는 node 삭제를 차단하지 않는다.

## 조회와 UI

Browser REST는 두 수준의 조회를 제공한다.

```text
GET  /api/v1/spaces/{space_id}/link-index
POST /api/v1/spaces/{space_id}/link-index/rebuild
GET  /api/v1/spaces/{space_id}/nodes/{node_id}/links
```

- Space Library의 Space Inspector는 상태와 링크 인덱싱 버튼만 제공한다. 기존 Space가 `uninitialized`이면 `Index links`, 이후에는 `Reindex links`로 표시한다.
- Node Inspector는 선택한 node의 outgoing, incoming, broken 관계를 bounded 목록과 count로 표시한다.
- 재인덱싱 요청은 비동기이며 현재 작업 상태를 반환한다.
- 이미 전체 재인덱싱이 실행 중이면 같은 요청은 추가 재인덱싱을 예약하지 않는다.
- 실패한 전체 재인덱싱에 대한 요청은 완료된 batch를 버리지 않고 즉시 재개한다.
- 관계 상태, count, bounded 목록, 표시 path는 하나의 repeatable-read snapshot에서 조회한다.

이번 계약에는 전체 Space 그래프 시각화, 외부 URL health check, AI 의미 관계, 관계 변경 이력, 링크 기반 삭제 차단을 포함하지 않는다.
