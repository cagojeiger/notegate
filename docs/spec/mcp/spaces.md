# MCP Spaces

Space 목록은 `read op=spaces`, Space lifecycle은 REST/dashboard user API가 제공한다.

| Caller | 목록과 target scope |
|---|---|
| User MCP | owner가 `user_mcp_enabled`로 설정한 Space |
| Agent MCP | Agent에 명시적으로 연결된 Space와 connection permission |

- User MCP에서 비활성화된 Space와 존재하지 않는 Space는 동일한 not-found 응답을 사용한다.

이 scope는 모든 MCP read/write/sequence/File tool의 target 해석과 진행 중 upload 재개에 적용한다. `navigation_pinned`는 dashboard 탐색 표시만 제어한다.

정본 contract는 [`tools.md`](./tools.md)를 따른다.
