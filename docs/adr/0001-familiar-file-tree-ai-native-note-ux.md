# ADR 0001: 익숙한 파일 트리 기반 AI 네이티브 노트 UX

## 상태

채택됨

## 맥락

notegate는 개인용 Markdown 노트 서비스다. 사용자는 새로운 정보 구조를 배우기보다
이미 익숙한 폴더, 문서, 이동, 이름 변경, 검색, 편집 개념으로 노트를 관리할 수 있어야 한다.

또한 AI agent는 제품의 부가 기능이 아니라 기본 사용 방식 중 하나다. 사람과 agent가 서로 다른
개념 체계를 쓰면 제품 사용성과 자동화 안정성이 모두 나빠진다.

## 결정

notegate의 기본 UX는 흔한 파일 트리 모델을 따른다.

제품은 workspace, folder, document, search, editor 중심의 mental model을 제공한다. 새로운
도메인 전용 구조를 만들기보다 익숙한 파일 관리 UX를 차용해 사용자와 AI agent 모두의 학습 비용을
낮춘다.

브라우저 UI와 LLM/CLI client는 같은 제품 개념을 공유하되, 각자에게 자연스러운 surface를 제공한다.
브라우저 UI는 화면 상태를 안정적으로 관리하기 쉬운 식별자 기반 경험을 우선하고, LLM/CLI는
workspace와 path 중심의 명령형 경험을 우선한다.

## 근거

파일 트리 UX는 대부분의 사용자가 이미 알고 있는 구조다. 학습 비용이 낮고, 노트 서비스의 문서
관리 경험과 잘 맞는다.

LLM도 파일 관리 workflow에 익숙하다. 따라서 별도의 제품 고유 개념을 길게 설명하는 것보다 파일
트리 기반 명령과 결과를 제공하는 편이 더 예측 가능하다.

브라우저와 LLM/CLI는 입력 방식이 다르지만, 사용자가 이해하는 제품 개념은 같아야 한다. Surface를
분리하더라도 같은 workspace, folder, document 개념을 공유하면 제품 동작을 일관되게 유지할 수 있다.

## 결과

- notegate는 개인용 Markdown 노트 서비스로 출발한다.
- workspace/folder/document/search/editor가 핵심 사용자 개념이 된다.
- 사람과 AI agent가 같은 파일 트리 mental model을 공유한다.
- REST는 UI 친화적인 surface, MCP는 LLM/CLI 친화적인 surface를 제공한다.
- 새롭고 낯선 정보 구조보다 익숙한 파일 관리 UX를 우선한다.
