# 마크다운 링크

NoteGate Markdown Text는 활성 Space 안의 node를 가리키는 링크에 대해 보수적인 GitHub 스타일 path 모델을 사용한다.

이 문서는 path 해석 규칙만 정의한다. backlink, Obsidian wikilink, title search, shortest-path lookup, cross-Space linking은 정의하지 않는다.

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
