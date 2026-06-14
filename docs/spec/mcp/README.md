# MCP tools

MCP는 agent/CLI용 target-first path API다. Tool은 파일 시스템 명령처럼 동작하되, Space lifecycle은 다루지 않는다. 여러 명령을 순서대로 실행할 때는 `run_sequence`를 사용한다.

```text
target = space:/absolute/path
```

Space name은 Unicode를 허용하지만 `target` 파싱을 위해 `:`는 사용할 수 없다.

노출되는 tool은 다음 6개다.

```text
me      caller identity 확인
read    spaces/ls/tree/stat/read
search  find/grep
write   write/append/patch/edit
manage  mkdir/mv/cp/rm
run_sequence  ordered command sequence 실행
```

MCP는 space create/delete/rename, agent 관리, API key 관리를 제공하지 않는다. 이 작업은 REST/dashboard user-only API에서 한다.

정본 tool contract는 [`tools.md`](./tools.md)를 따른다.
