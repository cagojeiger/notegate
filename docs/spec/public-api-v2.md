# V2 공개 API

V2는 외부 확장 서버가 사용하는 API key 전용 공개 계약이다. 브라우저 UI의 화면 구성에 필요한 V1 helper와 MCP의 path-first command를 그대로 노출하지 않는다.

## 인증

```http
Authorization: Bearer ngk_v1_...
```

- Browser session cookie와 OAuth JWT는 V2 인증 수단이 아니다.
- API key 문자열의 `v1`은 key 형식 version이며 HTTP API version과 무관하다.
- V2 응답은 `Cache-Control: private, no-store`로 전달한다.

## 초기 범위

V2의 초기 범위는 API key caller 확인만 제공한다.

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/api/v2/me` | API key caller 확인 |

Space, Node, Text, Search, preview, file transfer, event sync, mutation은 초기 V2에 포함하지 않는다.

## 운영 경계

V1, V2, MCP는 각각 독립된 in-process rate-limit bucket을 사용하고 전체 ingress hard limit을 공유한다. 이 제한은 계정이나 tier quota가 아니라 process-wide 안전 상한이다. 향후 V2를 별도 프로세스로 분리할 때도 public path와 DTO 계약은 유지하고, domain authorization과 invariant는 같은 service layer를 사용한다.

브라우저 세션 인증, V2 API key 인증, MCP 인증은 서로 다른 transport middleware에서 처리한다. API key hash와 live credential 판정만 공용 검증 경로를 사용한다.

OpenAPI가 활성화된 배포에서는 JSON을 `/openapi/v2.json`, Swagger UI를 `/swagger-ui/v2`에서 제공한다.
