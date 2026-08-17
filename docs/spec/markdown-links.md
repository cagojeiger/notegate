# 마크다운 링크

NoteGate Markdown Text는 활성 Space 안의 node를 가리키는 링크에 대해 보수적인 GitHub 스타일 path 모델을 사용한다.

이 문서는 path 해석 규칙과 내부 링크 관계 인덱스를 정의한다. Obsidian wikilink, title search, shortest-path lookup, cross-Space linking은 정의하지 않는다.

## 링크 종류

```text
[same folder](note.md)        -> 현재 문서의 폴더 기준 상대 경로
[child](./Policies/A.md)      -> 현재 문서의 폴더 기준 상대 경로
[parent](../README.md)        -> 현재 문서의 폴더 기준 상대 경로
[root](/README.md)            -> 활성 Space root 기준 절대 경로
[#section](#section)          -> 현재 문서 anchor, node resolve 대상 아님
[web](https://example.com)    -> 외부 URL, node resolve 대상 아님
```

규칙:

- 일반 상대 경로, `./`, `../`는 현재 문서의 parent folder 기준으로 resolve한다.
- 앞에 `/`가 붙은 path는 browser host가 아니라 활성 Space root 기준으로 resolve한다.
- URL-encoded path 문자는 Space path lookup 전에 path segment 단위로 decode한다. 예를 들어 `%20`은 space가 되고, `%23`은 `#`가 된다.
- Encoded slash가 decode되어 segment 안에 `/`를 만들면 invalid로 간주한다. Link text가 path separator를 숨겨 Space path 경계를 바꾸면 안 된다.
- Decode된 segment에 control character가 있으면 invalid로 간주한다.
- `.`과 `..` segment는 lookup 전에 normalize한다.
- 활성 Space root보다 위로 벗어나는 path는 invalid로 간주하고 node로 resolve하지 않는다.
- file path 뒤의 fragment identifier는 node lookup에서는 무시한다. 열린 문서 내부의 anchor navigation은 이 문서의 범위 밖이다.
- query string이 있는 링크는 node로 resolve하지 않는다.
- `http:`, `https:`, `mailto:`, `tel:` protocol link와 `//...` 형태의 protocol-relative link는 node로 resolve하지 않고 browser/external link로 유지한다.
- `javascript:`, `data:`, `blob:` 등 allowlist에 없는 protocol은 node로 resolve하지 않고 rendered href도 제거한다.

## Resolve 동작

Frontend는 REST path endpoint로 node link를 resolve한다.

```http
GET /api/v1/spaces/{space_id}/paths/resolve?path=/folder/note.md
```

Resolve에 성공하면 client는 반환된 node를 일반 workbench node-open flow로 열고, file tree에서 ancestor를 reveal한다.

Resolve에 실패하면 현재 문서는 그대로 유지하고 client는 비파괴적인 error를 표시한다. Client는 title을 추정하거나, file extension을 임의로 붙이거나, 다른 folder를 search하거나, Space를 전환하면 안 된다.

Plain click에서 내부 path 후보이지만 invalid인 링크는 browser navigation으로 넘기지 않는다. 현재 문서를 유지하고 `Invalid markdown link` toast를 표시한다.

Modifier click 또는 non-primary click은 client가 가로채지 않고 browser 기본 동작에 맡긴다.

## 이미지 링크

Markdown image도 같은 path 해석 규칙을 사용한다.

```text
![same folder](image.png)          -> 현재 문서의 폴더 기준 상대 경로
![child](./Assets/diagram.png)     -> 현재 문서의 폴더 기준 상대 경로
![parent](../Assets/logo.png)      -> 현재 문서의 폴더 기준 상대 경로
![root](/Assets/logo.png)          -> 활성 Space root 기준 절대 경로
![web](https://example.com/a.png)  -> 외부 URL, node resolve 대상 아님
```

규칙:

- 표준 Markdown image syntax인 `![alt](path)`만 정의한다.
- 내부 image path는 link path와 동일하게 normalize하고 REST path endpoint로 resolve한다.
- Resolve된 node가 10 MiB 이하 `file`이고 client-encrypted file이 아니며, 서버가 실제 bytes를 PNG, JPEG, WebP, AVIF, GIF 중 하나로 감지했을 때만 preview 안에 image로 표시한다.
- 내부 image는 viewport에 가까워졌을 때 짧게 만료되는 file preview URL을 발급받는다. URL로 image를 불러오지 못하면 새 URL을 한 번만 발급받아 재시도한다. Client 선언 `media_type`과 파일 확장자는 inline 표시 여부를 결정하지 않는다.
- Resolve 실패, invalid path, file이 아닌 node, 지원하지 않는 형식, client-encrypted file, 10 MiB 초과 file은 현재 문서를 유지하고 preview 안에 비파괴적인 placeholder를 표시한다. SVG와 PDF는 image preview 대상이 아니다.
- 외부 `http:`, `https:` image는 자동으로 요청하지 않는다. 사용자가 placeholder를 눌렀을 때만 `Referer` 없이 불러온다.
- `javascript:`, `data:`, `blob:` 등 allowlist에 없는 protocol과 protocol-relative URL(`//example.com/a.png`)은 rendered `src`를 제거하고 image로 load하지 않는다.

Obsidian wikilink embed syntax인 `![[image.png]]`, width syntax인 `![[image.png|300]]`, vault-wide filename lookup, attachment folder 자동 탐색은 이 문서에서 정의하지 않는다.

## 링크 관계 인덱스

Backend는 Text의 표준 Markdown link와 image destination을 파싱해 같은 Space 안의 관계를 비동기로 projection한다. 원문 Text가 관계의 source of truth이며, 각 source node의 outgoing 관계 전체를 최신 본문 기준으로 교체한다. Incoming 관계는 별도 복제하지 않고 resolved target node index로 조회한다.

관계 row는 다음 정보만 보관한다.

- source node id
- 같은 Space에서 resolve된 target node id 또는 `null`
- 정규화된 target path
- `link` 또는 `image` 종류
- 같은 source 안의 occurrence count

Target이 없거나 삭제되면 target node id는 `null`이지만 path는 유지한다. 따라서 깨진 링크를 조회할 수 있고, 같은 path에 node가 다시 생성된 뒤 source를 projection하면 새 node id로 연결된다. Text 본문은 관계 테이블에 복제하지 않는다. Server at-rest encryption을 사용하는 plain Text는 application service에서 복호화한 뒤 파싱한다. Client-encrypted Text는 server가 본문을 읽을 수 없으므로 관계를 만들지 않으며, 기존 projection이 있으면 제거한다.

문서 변경 트랜잭션은 해당 Space에 등록된 비동기 processor를 pending 상태로 전환한다. Link processor는 pending Space만 골라 Space별 checkpoint 이후의 `file_change_events`를 읽고, 중복 source id를 durable projection target으로 합친다. 새 processor의 첫 실행과 checkpoint event가 retention에서 사라진 경우에는 전체 Space를 스캔해 기준 상태를 만든다. 전체 스캔은 후보 등록을 pass당 최대 500개로 제한하고 durable node cursor에서 이어간다. 스캔 시작 시 event 경계를 저장하므로 실행 중 발생한 변경은 완료 후 증분 처리된다. Target 등록과 checkpoint 갱신은 같은 transaction에서 완료되며, background job은 target을 실행할 뿐 source of truth가 아니다. 준비된 target은 transaction당 최대 500개를 옮기고, Space별 최대 50개 단위의 작업으로 queue에 등록한다. Link collector가 backlog를 발견하면 lock을 해제한 뒤 1초 후 다음 bounded pass를 실행한다. Queue worker의 concurrency가 실제 병렬 실행량을 제한한다. Background queue가 잡 하나의 제한된 자동 재시도를 전담하고, 최종 실패한 target은 실패 상태로 남긴다. 새 변경·수동 동기화·전체 재색인은 해당 target의 실패 상태를 초기화하고 다시 활성화한다. 모든 Space를 주기적으로 순회하지 않는다.

생성, 이름 변경, 이동, 복사는 path resolve 결과에 영향을 줄 수 있으므로 해당 Space의 live Text와 기존 source projection을 target으로 등록한다. 삭제는 soft-deleted subtree id를 target으로 등록해 outgoing 관계를 제거하고 incoming 관계를 broken 상태로 바꾼다. 실행 중 source가 다시 변경되면 이전 job은 최신 target을 완료 처리하지 않고 해제하며, 준비된 최신 version을 새 작업으로 등록한다. Lease가 만료된 attempt는 동일 job id라도 claim token이 다르므로 관계를 갱신할 수 없다.

수동 요청도 같은 background job 경로를 사용한다.

```http
POST /api/v1/spaces/{space_id}/nodes/{node_id}/links/sync
POST /api/v1/spaces/{space_id}/link-index/reindex
```

조회는 browser session과 기존 Space permission을 그대로 적용한다.

```http
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/incoming
```

Projection은 eventual consistency 모델이다. 본문 저장 성공과 링크 관계 갱신 완료 사이에는 지연이 있을 수 있다. Node link 상태 응답은 현재 동기화 상태, 마지막 성공 시각, 최종 실패 코드를 제공한다. 관계 교체, 해당 시각 갱신, 처리한 target 완료는 하나의 database transaction으로 완료한다.
