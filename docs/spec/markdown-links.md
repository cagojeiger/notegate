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
- 관계 전체 교체 transaction은 도메인 변경을 준비한 뒤 commit 직전에 현재 claim token을 잠근다. 긴 작업 중에는 heartbeat가 lease를 갱신할 수 있고, claim을 잃은 worker의 도메인 변경은 전부 rollback된다.
- 타깃 경로 해석과 outgoing 전체 교체는 파일 트리 변경과 같은 Space 잠금을 잡은 최종 transaction에서 수행한다. 타깃 이동, 이름 변경, 삭제와 오래된 worker 결과의 commit 순서가 하나로 직렬화된다.
- 관계 교체가 commit된 뒤 queue 성공 기록 전에 worker가 종료될 수 있다. 이 경우 같은 작업을 다시 실행하며, source 전체 교체가 멱등성을 보장한다.
- 같은 문서가 처리 중 다시 변경되면 새 요청이 남는다. 이전 worker가 완료되어도 새 요청은 지워지지 않으며 현재 본문으로 다시 처리된다.
- 같은 source, 영향 범위 또는 Space에 아직 실행되지 않은 fresh 요청이 있으면 database unique key가 중복 요청을 그 작업으로 병합한다. 등록과 영향 분석은 Space write lock을 잡지 않는다. 실행 중인 작업이나 재시도 작업은 새 변경을 흡수하지 않으므로 fresh 후속 작업을 별도로 남긴다.
- Worker claim은 fencing token으로 보호한다. Queue 상태 전이와 링크 관계 교체가 모두 현재 token을 확인한다.
- Parser 규칙이 바뀌면 `LINK_PARSER_VERSION`을 올리고 같은 배포의 새 migration에서 모든 live Space 재인덱싱을 요청한다. 이전 parser version의 source 상태는 동기화 완료로 간주하지 않는다.
- 문서·파일·폴더의 생성, 이름 변경, 이동, 복사, 삭제는 변경 node 기준의 영향 분석 작업을 요청한다. 이 작업은 변경 subtree의 text, 해당 subtree node를 가리키던 source, 새 경로와 일치하는 broken 관계의 source만 문서별 작업으로 전개한다.
- `target_node_id`는 구조 변경의 영향 source를 찾기 위한 내부 anchor다. Markdown path가 이동 후에도 유효한 것으로 간주하지 않으며, source를 다시 파싱해 현재 path 규칙으로 resolve한다.
- Space 전체 재인덱싱은 migration 초기화, 새 Space 초기화, parser version 변경, 사용자 요청, 복구에만 사용한다. 현재 존재하는 모든 text node와 관계가 남은 삭제 source를 문서별 작업으로 전개한다. 전체 작업은 관계를 직접 변경하지 않으며 source 작업만 관계와 source 상태를 교체하거나 삭제한다.
- 목적지 node를 찾지 못한 내부 경로도 `target_node_id = null`인 broken 관계로 보존한다. 이후 경로 구조가 바뀌면 재인덱싱으로 다시 resolve한다.
- Server 저장 암호화 문서는 worker가 복호화해 인덱싱한다. 저장 암호화 전환은 plaintext hash와 path를 바꾸지 않으므로 별도 인덱싱 요청을 만들지 않는다. Client 암호화 문서는 서버가 본문을 읽을 수 없으므로 outgoing 관계를 만들지 않는다.
- Node Inspector에서는 outgoing, incoming, broken 관계와 동기화 상태를 보여준다. 내부 revision 값은 사용자에게 노출하지 않는다.
- Outgoing과 incoming 목록은 각각 독립적인 opaque cursor로 page를 이어 읽는다. 한 응답의 page size에는 상한이 있지만 전체 관계 수에는 별도 조회 상한을 두지 않는다.
- 사용자는 text node를 수동 동기화하거나 Space 전체 재인덱싱을 요청할 수 있다. 요청은 background worker가 처리하며 화면 요청이 완료될 때까지 HTTP 연결을 유지하지 않는다.

관계 인덱스는 결과적 일관성을 사용한다. 문서 저장 직후에는 이전 관계가 잠시 보일 수 있지만, 대기 중인 요청이 모두 처리되면 현재 문서와 일치해야 한다.

Space 상태는 보존 기간이 있는 queue row만으로 freshness를 판단하지 않는다. 현재 live tree의 source 본문 hash와 path, parser version, resolve된 target path를 마지막 projection과 비교한다. 따라서 실패한 queue 기록이 정리된 뒤에도 오래된 projection은 `up_to_date`가 되지 않는다. `outdated_documents`는 현재 projection과 일치하지 않는 문서 수다. 활성 작업이 있으면 `pending`, `syncing`, `retrying` 중 하나이며, 오래된 projection에 활성 복구 작업이 없을 때만 `failed`다. API의 `latest_index_update_at`은 최근 source projection 또는 source가 없는 Space의 작업 전개 시각이며 전체 Space 동기화 완료 시각이 아니다.

Dashboard 조회 경로는 Space 상태와 node 관계 역할로 분리한다. Node Inspector와 Space Library는 같은 Space 상태 query/cache를 공유한다.

```text
GET /api/v1/spaces/{space_id}/link-index                      # Space 동기화 상태
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing  # cursor page
GET /api/v1/spaces/{space_id}/nodes/{node_id}/links/incoming  # cursor page
```

## Background runtime과 확장

- 인덱싱 요청은 원본 데이터와 같은 PostgreSQL database의 `background_jobs`에 기록한다. 별도 queue database는 사용하지 않는다. 별도 database를 쓰면 원본 변경과 작업 등록을 한 transaction으로 보장할 수 없기 때문이다.
- API process가 HTTP와 background handler를 함께 실행하며 하나의 database connection pool을 공유한다. `NOTEGATE_BACKGROUND_JOBS__CONCURRENCY`는 process별 handler 동시 실행 상한이다.
- Migration과 crypto key epoch 초기화가 완료된 뒤 API가 background runtime을 시작한다. 준비되지 않은 database에서는 HTTP만 부분 기동하지 않는다.
- Commit된 요청은 PostgreSQL `NOTIFY`로 consumer를 깨운다. 알림은 작업 원장이 아니며 LISTEN 연결에 실패하거나 알림이 유실되어도 10분 기준 ±10% safety poll로 queue를 다시 확인한다.
- 여러 API replica의 consumer는 `FOR UPDATE SKIP LOCKED`로 bounded batch를 선점하고 process별 concurrency 상한 안에서 실행한다.
- KEDA는 read-only database account로 `SELECT background_job_backlog(NULL)`의 단일 숫자를 조회한다. 이 값은 지금 실행할 수 있거나 실행 중인 작업 수이며, 재시도 시각이 아직 오지 않은 작업은 제외한다.
- HTTP와 worker runtime이 같은 process이므로 KEDA는 NoteGate replica 전체를 확장한다. 새 작업만을 이유로 HTTP process를 별도 확장해야 하는 규모가 되면 동일한 crate 경계를 유지한 채 worker-only deployment를 다시 분리할 수 있다.

범용 queue의 상태 머신, 재시도, 보관 및 관측 계약은 `background-jobs.md`를 따른다.
