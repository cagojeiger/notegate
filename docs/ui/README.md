# UI 문서

NoteGate 대시보드의 현재 화면 구조와 동작을 기록한다.

문서별 소유 범위:

- [`DESIGN.md`](../../DESIGN.md): 제품의 디자인 원칙과 사용자 문구
- `00-overview.md`: 화면과 용어의 전체 구조
- `01-layout.md`: layout, panel과 반응형 규칙
- `02-data-and-flows.md`: 상태 수명, 사용자 흐름과 실패 처리
- `03-implementation.md`: frontend 구현 경계와 검증 규칙
- [`docs/spec`](../spec/api.md): API, 보안, 저장소와 성능 계약

실제 동작, 상수와 token의 정본은 코드다. 문서와 코드가 다르면 현재 코드를 확인하고 같은 변경에서 관련 문서를 맞춘다.

규칙:

- 문서는 한글로 작성한다.
- 현재 구현과 유지할 규칙만 기록한다.
- 변경 이력, 완료 보고와 계획은 기록하지 않는다.
- 같은 규칙을 여러 문서에 복사하지 않고 소유 문서로 연결한다.

읽는 순서:

1. `00-overview.md`
2. `01-layout.md`
3. `02-data-and-flows.md`
4. `03-implementation.md`
