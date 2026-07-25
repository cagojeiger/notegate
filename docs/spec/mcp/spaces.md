# MCP Spaces

MCP는 Space 목록 조회만 제공한다.

- Space 목록: `read` tool의 `op=spaces`
- Space create/delete/rename: MCP에서 제공하지 않으며 REST/dashboard에서만 수행한다.
- User MCP: owner가 `user_mcp_enabled`로 설정한 Space만 목록과 실제 접근 범위에 포함한다.
- Agent MCP: `user_mcp_enabled`를 사용하지 않는다. Agent에 명시적으로 연결된 Space만 permission에 따라 접근한다.
- User MCP에서 비활성화된 Space와 존재하지 않는 Space는 동일한 not-found 응답을 사용한다.

이 필터는 목록 표시 옵션이 아니다. `read`, `search`, `write`, `manage`, `run_sequence`, `file_transfer`의 target 해석과 진행 중 upload 재개에도 같은 범위를 적용한다. `navigation_pinned`는 MCP 권한에 영향을 주지 않는다.

정본 contract는 [`tools.md`](./tools.md)를 따른다.
