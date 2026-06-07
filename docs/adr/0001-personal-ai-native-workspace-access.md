# ADR 0001: 개인용 AI 네이티브 워크스페이스 접근 모델

## 상태

채택됨

## 맥락

notegate는 개인용 Markdown 노트 서비스다. 사용자는 익숙한 파일/문서 도구처럼
workspace, folder, document, search, editor 중심의 역할을 사용해야 한다.

제품의 기본 UX는 흔한 파일 트리 모델을 따른다. 많은 사용자가 폴더, 문서, 이동, 이름 변경,
검색 같은 개념에 이미 익숙하고, 최신 LLM도 터미널/파일 관리 workflow를 잘 학습하고 있다.
따라서 notegate는 새로운 정보 구조를 만들기보다 익숙한 파일 관리 UX를 차용해 사용자와
agent 모두의 학습 비용을 낮추는 방향을 택한다.

동시에 AI agent는 제품의 부가 기능이 아니라 1급 사용 방식이다. 사용자는 agent를
workspace에 연결해 읽기 전용 또는 편집 workflow를 맡길 수 있어야 하고, 현재 문서/노드 상태는
마지막으로 사람 사용자가 바꾼 것인지 agent가 바꾼 것인지 구분할 수 있어야 한다.

인증 방식과 실제 행동 주체는 분리해서 본다.

```text
브라우저 로그인 via authgate      -> user account
MCP OAuth 2.1 via authgate       -> user account
device flow via authgate         -> user account
API key / agent key              -> agent account
```

OAuth/authgate 계열은 사람 사용자 신원을 증명한다. API key는 장기 자동화 credential이므로,
사용자가 만든 key라도 호출 주체는 agent로 취급한다.

## 결정

초기 접근 제어 단위는 workspace로 한다. 파일/폴더/node 단위 ACL은 도입하지 않는다.

역할은 사용자가 이해하기 쉬운 공유 UX 기준으로 정한다.

```text
viewer = 목록/메타데이터/읽기/검색
editor = viewer + 생성/수정/패치/이동/삭제
owner  = editor + workspace 접근 권한과 agent key 관리
```

workspace는 단일/default 제약 없이 자유롭게 생성/삭제할 수 있는 1급 리소스다.
workspace 생성자는 자동으로 `owner`가 된다. Agent는 workspace에 별도로 연결되며,
일반적으로 `viewer` 또는 `editor` 역할을 받는다.

사람 사용자와 agent는 모두 공통 actor identity를 가진다. 따라서 생성자, 수정자,
삭제자는 user/agent 여부와 관계없이 같은 방식으로 참조한다.

사용자 탈퇴나 agent 삭제는 hard delete가 아니라 비활성화/soft delete로 처리한다.
과거 문서의 생성자/수정자/삭제자 참조가 깨지면 안 되기 때문이다.

## 근거

workspace 단위 권한은 개인용 노트 서비스라는 제품 철학과 잘 맞는다. 사용자는 agent를
개별 파일마다 연결하기보다 자신의 note workspace에 연결한다고 이해하는 편이 자연스럽다.

파일 트리 UX는 사용자가 이미 알고 있는 mental model을 재사용한다. 또한 LLM에게도
`ls`, `read`, `write`, `patch`, `mv`, `grep` 같은 파일 관리 방식은 친숙한 작업 단위다.
이는 제품 고유 개념을 학습시키는 비용을 줄이고, 사람과 agent가 같은 구조를 공유하게 한다.

검색 성능 면에서도 workspace 단위 권한이 단순하다. 요청 시작 시 한 번 권한을 확인하면,
`find`나 `grep` 쿼리는 `workspace_id` 기준으로 실행할 수 있다. 파일별 ACL을 넣으면 검색마다
권한 join, 상속 계산, ACL cache가 필요해진다.

Unix-style group, inherited permission, file/folder ACL은 협업 제품에서는 유용하지만,
초기 notegate의 개인용 AI-native 모델에는 과하다. 필요해지면 별도의 주요 기능으로
성능 설계와 UX를 함께 다시 결정한다.

역할 이름은 `reader`/`writer` 같은 기술적 이름보다 `viewer`/`editor`/`owner`를 쓴다.
일반적인 문서 공유 UX에 더 가깝기 때문이다.

## 결과

- 검색은 workspace-scoped로 유지된다.
- 권한은 deny-by-default다. 접근 row가 없으면 접근할 수 없다.
- API key는 agent credential이며, 사람 owner 계정과 분리해서 revoke할 수 있다.
- 사용자/agent 삭제 이후에도 과거 `created_by`, `updated_by`, `deleted_by` 참조가 유지된다.
- REST는 UI 친화적인 식별자 기반 surface를 제공한다.
- MCP는 LLM/CLI 친화적인 workspace/path 기반 surface를 제공한다.
- audit log/event history는 이번 단계에서 제외한다. 필요해지면 별도 결정으로 추가한다.
- 파일/폴더 단위 공유는 보류한다.
