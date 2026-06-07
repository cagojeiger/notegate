# ADR 0002: 사용자, AI agent, workspace 접근 모델

## 상태

채택됨

## 맥락

notegate에서는 사람 사용자뿐 아니라 AI agent도 문서를 읽고 수정할 수 있다. 따라서 어떤 작업을
사람이 했는지, agent가 했는지 구분할 수 있어야 한다.

사용자는 보통 개별 파일마다 권한을 설정하기보다 자신의 노트 workspace에 사람이나 agent를 연결한다고
이해하는 편이 자연스럽다. 파일/폴더 단위 ACL이나 Unix-style permission은 강력하지만, 개인용
AI-native 노트 서비스의 초기 UX에는 과하다.

## 결정

사람 사용자와 AI agent를 모두 공통 actor로 취급한다. 문서, 폴더, workspace의 생성자와 마지막
수정자, 삭제자는 user/agent 여부와 관계없이 같은 방식으로 참조한다.

인증 방식별 행동 주체는 다음 원칙을 따른다.

```text
브라우저 로그인 via authgate      -> user
MCP OAuth 2.1 via authgate       -> user
device flow via authgate         -> user
API key / agent key              -> agent
```

사용자가 만든 key라도 API key로 호출하면 행동 주체는 agent로 본다.

접근 제어 단위는 workspace로 한다. 파일, 폴더, node 단위 ACL은 도입하지 않는다.

역할은 일반적인 문서 공유 UX에 맞춰 다음 세 가지로 둔다.

```text
viewer = 목록/메타데이터/읽기/검색
editor = viewer + 생성/수정/패치/이동/삭제
owner  = editor + workspace 접근 권한 관리
```

workspace는 단일/default 제약 없이 자유롭게 생성/삭제할 수 있는 1급 리소스다. workspace 생성자는
자동으로 owner가 된다. Agent는 workspace에 별도로 연결되며, 보통 viewer 또는 editor 역할을 받는다.

사용자 탈퇴나 agent 삭제는 hard delete가 아니라 비활성화/soft delete로 처리한다. 과거 문서의
생성자/수정자/삭제자 참조가 깨지면 안 되기 때문이다.

## 근거

AI agent가 1급 사용 방식이면 agent의 행동을 사람 사용자 행동과 구분해 추적할 수 있어야 한다.
그래야 사용자가 문서 상태를 신뢰하고, agent 접근을 독립적으로 회수할 수 있다.

API key를 사람 사용자로 취급하면 자동화 credential과 사람 세션의 경계가 흐려진다. API key는
agent credential로 취급하는 편이 권한 회수와 운영이 단순하다.

workspace 단위 권한은 사용자가 이해하기 쉽고, agent 연결 UX와도 잘 맞는다. 사용자는 “이 agent가
내 workspace를 읽게 한다/편집하게 한다”라고 이해할 수 있다.

검색 성능 면에서도 workspace 단위 권한이 단순하다. 파일별 ACL을 넣으면 검색마다 권한 상속과
필터링이 필요해진다.

## 결과

- user와 agent는 모두 행동 주체가 될 수 있다.
- API key는 agent credential로 취급한다.
- 권한은 deny-by-default다. workspace 접근 권한이 없으면 접근할 수 없다.
- 파일/폴더 단위 공유는 보류한다.
- 역할 이름은 기술적 permission보다 사용자가 익숙한 viewer/editor/owner를 사용한다.
- 과거 attribution 보존을 위해 user/agent는 일반 product action으로 hard delete하지 않는다.
- audit log/event history는 이번 단계에서 제외한다. 필요해지면 별도 결정으로 추가한다.
