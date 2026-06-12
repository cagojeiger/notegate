# ADR 0001: AI-native personal file space

## Context

notegate는 개인이 소유하고 AI agent가 함께 사용하는 중앙 저장소다. 사용자는 익숙한 파일/폴더 UX로 정보를 관리하고, agent는 같은 tree를 API/MCP로 읽고 쓴다.

notegate의 제품 경계는 개인 저장소다. Team/organization 협업 모델은 이 제품 라인의 범위에 포함하지 않는다. 한 사람이 서로 다른 로그인 신원으로 접근하면 별개 user로 취급하며, 계정 병합도 범위에 포함하지 않는다.

## Decision

제품의 핵심 개념은 `Space / Folder / Text / File`이다.

```text
Space  = user가 소유한 저장 범위
Folder = tree container
Text   = UTF-8 content; read/write/append/patch/grep 가능
File   = binary/object content; Text content operation과 grep 대상이 아님
```

사람 UX는 파일 시스템처럼 보이게 하고, agent UX는 text와 file의 가능한 작업을 명확히 구분한다.

## Consequences

- `Markdown document`를 제품의 중심 개념으로 두지 않는다.
- Markdown, JSON, JSONL, YAML, TXT 등은 모두 `Text`다.
- 이미지, 음성, PDF, 압축 파일 등은 `File`이다.
- 검색 본문 검색은 `Text`만 대상으로 한다.
- `File`에서 OCR, transcription, embedding 등 파생 text를 만들 수 있지만 원본 file과 별도 lifecycle로 다룬다.
