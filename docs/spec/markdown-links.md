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

Frontend는 기존 REST path endpoint로 node link를 resolve한다.

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

## 관계 인덱스

NoteGate는 표준 Markdown link와 image의 내부 경로를 문서별 관계로 비동기 인덱싱한다.

```text
문서 저장 ── 같은 transaction ──> 변경 이벤트 + 문서 인덱싱 요청
                                      │
                                      ▼
                               background worker
                                      │
                                      ▼
                           해당 문서의 outgoing 전체 교체
```

규칙:

- 관계의 소유 단위는 source 문서다. Worker는 source 문서의 현재 본문을 읽고 기존 outgoing 관계를 새 결과로 전체 교체한다.
- incoming 관계는 별도로 복제하지 않고 `target_node_id`의 역방향 조회로 구한다.
- 문서 변경과 인덱싱 요청은 같은 database transaction에 기록한다. 둘 중 하나가 실패하면 문서 변경도 commit하지 않는다.
- 관계 전체 교체와 해당 요청의 적용 완료 표시는 같은 database transaction에서 처리한다.
- 같은 문서가 처리 중 다시 변경되면 새 요청이 남는다. 이전 worker가 완료되어도 새 요청은 지워지지 않으며 현재 본문으로 다시 처리된다.
- Worker claim은 fencing token으로 보호한다. 만료된 worker는 이후 worker가 선점한 결과를 덮어쓸 수 없다.
- 문서·파일·폴더의 생성, 이름 변경, 이동, 복사, 삭제는 경로 해석 결과에 영향을 주므로 Space 재인덱싱을 요청한다.
- Space 재인덱싱은 현재 존재하는 모든 text node를 문서별 작업으로 전개한다. 삭제된 source의 관계와 작업 상태는 정리한다.
- 목적지 node를 찾지 못한 내부 경로도 `target_node_id = null`인 broken 관계로 보존한다. 이후 경로 구조가 바뀌면 재인덱싱으로 다시 resolve한다.
- Server 저장 암호화 문서는 worker가 복호화해 인덱싱한다. Client 암호화 문서는 서버가 본문을 읽을 수 없으므로 outgoing 관계를 만들지 않는다.
- Node Inspector에서는 outgoing, incoming, broken 관계와 동기화 상태를 보여준다. 내부 revision 값은 사용자에게 노출하지 않는다.
- 사용자는 text node를 수동 동기화하거나 Space 전체 재인덱싱을 요청할 수 있다. 요청은 background worker가 처리하며 화면 요청이 완료될 때까지 HTTP 연결을 유지하지 않는다.

관계 인덱스는 결과적 일관성을 사용한다. 문서 저장 직후에는 이전 관계가 잠시 보일 수 있지만, 대기 중인 요청이 모두 처리되면 현재 문서와 일치해야 한다.
