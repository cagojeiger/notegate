# REST Links

Link API는 표준 Markdown link와 image에서 비동기로 계산한 같은 Space 관계를 조회하고 재계산을 요청한다. Path 해석과 projection 규칙은 [`../markdown-links.md`](../markdown-links.md)가 정본이다.

## Node projection state

```http
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links
```

Space read 권한이 필요하다. `status`는 `idle`, `pending`, `syncing`, `failed` 중 하나다. `space_pending`은 아직 분류되지 않았거나 완료되지 않은 Space 단위 변경이 있음을 뜻하며, `status`가 `idle`이어도 갱신 중으로 해석한다. Collector가 아직 Space 변경을 분류하지 않았으면 live Text는 보수적으로 `pending`이다. 새 변경이 기존 실패를 대체할 수 있으므로 `pending`과 `syncing`이 이전 `failed`보다 우선한다. `projected_at`은 이 source Text의 outgoing 관계 전체가 마지막으로 교체된 시각이며, 아직 계산되지 않았으면 `null`이다. 실패해도 마지막 성공 projection은 유지된다. `failure_code`와 `failed_at`은 `status`가 `failed`일 때만 존재한다. Folder와 file도 조회할 수 있지만 source projection은 Text에만 존재한다.

```json
{
  "status":"idle",
  "space_pending":false,
  "projected_at":"2026-08-17T12:00:00Z",
  "failure_code":null,
  "failed_at":null,
  "availability":{"can_trigger":true,"reason":null,"retry_at":null}
}
```

## Outgoing and incoming

```http
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing?limit=50&cursor=...
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/incoming?limit=50&cursor=...
```

Space read 권한이 필요하다. 기본 page size는 50, 최대 100이다. `cursor`는 opaque 값이며 client는 해석하지 않는다.

```json
{
  "links": [
    {
      "node_id": "target-node-id",
      "path": "/docs/target.md",
      "kind": "link",
      "occurrence_count": 2
    }
  ],
  "page": {"limit":50,"returned":1,"has_more":false,"next_cursor":null}
}
```

Outgoing의 `node_id`는 target이 없거나 삭제되었으면 `null`이다. Incoming은 현재 조회 가능한 live source의 현재 path를 반환한다.

## Manual synchronization

```http
POST /api/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links
POST /api/v1/spaces/{space_id}/actions/reindex-links
GET /api/v1/spaces/{space_id}/link-index
```

Browser user와 Space write 권한이 필요하다. 첫 endpoint는 client-encrypted가 아닌 Text 하나를, 두 번째 endpoint는 Space의 live Text와 남아 있는 source projection을 비동기 처리 대상으로 접수한다. 실패한 target도 같은 요청으로 다시 활성화한다. 처리량 상한에 도달하면 projection 상태에 보관했다가 여유가 생길 때 background queue에 등록한다. 아직 job이 배정되지 않은 staged projection도 Space link index의 `pending` 상태에 포함된다. 같은 범위가 이미 staged/queued/running이면 새 job을 만들지 않고 `202`와 `result=already_pending`을 반환한다. Link projection은 여러 job으로 분할될 수 있으므로 command API는 job ID를 노출하지 않는다. GET Link index와 Node links는 domain 상태와 공통 `availability`를 반환해 다른 탭과 새로고침 후에도 같은 실행 가능 상태를 제공한다.

```json
{
  "result": "accepted",
  "availability": {"can_trigger": false, "reason": "pending", "retry_at": null}
}
```
