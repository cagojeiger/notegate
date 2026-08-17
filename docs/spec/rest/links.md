# REST Links

Link API는 표준 Markdown link와 image에서 비동기로 계산한 같은 Space 관계를 조회하고 재계산을 요청한다. Path 해석과 projection 규칙은 [`../markdown-links.md`](../markdown-links.md)가 정본이다.

## Node projection state

```http
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links
```

Space read 권한이 필요하다. `status`는 `idle`, `pending`, `syncing`, `failed` 중 하나다. `projected_at`은 이 source Text의 outgoing 관계 전체가 마지막으로 교체된 시각이며, 아직 계산되지 않았으면 `null`이다. 실패해도 마지막으로 성공한 projection은 유지된다. `failure_code`와 `failed_at`은 최신 요청이 최종 실패했을 때만 존재한다. Folder와 file도 조회할 수 있지만 source projection은 Text에만 존재한다.

```json
{
  "status":"idle",
  "projected_at":"2026-08-17T12:00:00Z",
  "failure_code":null,
  "failed_at":null
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
POST /api/v1/spaces/{space_id}/nodes/{node_id}/links/sync
POST /api/v1/spaces/{space_id}/link-index/reindex
```

Browser user와 Space write 권한이 필요하다. 첫 endpoint는 Text 하나를, 두 번째 endpoint는 Space의 live Text와 남아 있는 source projection을 background queue에 등록한다. 실패한 target도 같은 요청으로 다시 활성화한다. 둘 다 `202 Accepted`를 반환한다.

```json
{"status":"queued"}
```
