# ADR 0007: 사용자·Agent 코드 실행의 격리 경계

## 상태

제안. 이 문서는 목표 구조와 출시 조건을 정한다. 실행 API, 클러스터, 런타임은 아직 배포되지 않았다. 데이터가 자체 인프라 밖으로 나가도 되는지는 별도 결정이 필요하다. 아래 배치 설계는 자체 인프라 보관을 가정한다.

## 배경

NoteGate에는 사용자가 작성한 Python·CLI 코드와 AI Agent가 작성한 코드를 임시로 실행할 요구가 있다. 코드는 악의적일 수 있으므로 기존 API/worker Pod에서 실행하면 DB, S3, 암호화 키와 같은 운영 권한을 공유하게 된다. 현재 `notegate-jobs`는 at-least-once 전달과 자동 재시도를 제공하므로, 미확인 실행 결과를 가진 임의 코드를 그 handler에 넣을 수도 없다.

2026-09-28에 확인한 운영 OKE 클러스터는 ARM64, CRI-O 1.36, Flannel이며 `RuntimeClass`와 `NetworkPolicy` 리소스가 없다. [gVisor의 CRI-O 안내](https://gvisor.dev/docs/user_guide/containerd/crio/)는 CRI-O 연동이 비공식이고 최소 1.37을 요구하며 containerd를 권장한다. Kubernetes는 NetworkPolicy를 실제로 적용하는 CNI가 필요하다고 [명시한다](https://kubernetes.io/docs/concepts/services-networking/network-policies/). 이 상태를 안전한 비신뢰 코드 실행 기반으로 간주하지 않는다.

## 결정

사용자와 Agent는 같은 실행 계약과 격리 정책을 사용한다. NoteGate API가 요청자를 인증하고 실행 권한·소유자별 할당량·입력 범위를 검사한 뒤, 영속적인 `run_id`와 실행 원장을 기록한다. Agent의 Space 연결 또는 문서 읽기 권한만으로 코드 실행 권한을 추론하지 않는다. Agent에는 owner가 명시적으로 부여한 실행 권한이 필요하다. 클라이언트 측 암호화 콘텐츠는 서버가 복호화하지 않으며, 사용자가 명시적으로 제공한 입력 스냅샷만 실행에 전달한다.

전용 실행 서비스가 별도 샌드박스 클러스터를 관리한다. 이 서비스만 Kubernetes API에 접근하고, 승인된 이미지와 자원 프로필로 실행 인스턴스를 만든다. NoteGate의 일반 API·worker에는 Kubernetes 자격 증명을 주지 않는다. 실행 서비스는 입력을 직접 전송하고 제한된 출력만 회수한다. 실행 인스턴스에는 NoteGate/클라우드 자격 증명, Kubernetes 서비스 계정 토큰, 운영 볼륨을 전달하지 않는다.

```text
User / Agent
     │
     ▼
NoteGate API ── 인증·권한·할당량 ── 실행 원장(run_id)
     │                                     ▲
     │ private authenticated request       │ 상태·결과
     ▼                                     │
Execution service ── Kubernetes API ── Agent Sandbox controller
     │                                     │
     │ 입력 전달·명령 실행·출력 회수         ▼
     └─────────────────────────────── 격리된 Sandbox Pod
                                     gVisor + 제한된 자원·네트워크
```

샌드박스 클러스터의 목표 조합은 containerd, gVisor `RuntimeClass`, NetworkPolicy를 집행하는 CNI다. Kubernetes SIG의 [Agent Sandbox](https://github.com/kubernetes-sigs/agent-sandbox)를 생명주기 컨트롤러 후보로 채택하고, 짧은 수명의 `SandboxClaim`/서버 소유 `SandboxTemplate`과 비공개 `sandboxd` 실행 API를 검증한다. 기존 제품이 Pod 생성·삭제·만료를 담당하므로 같은 기능의 자체 오퍼레이터를 만들지 않는다. Agent Sandbox는 격리 런타임 자체가 아니며, [공식 위협 모델](https://github.com/kubernetes-sigs/agent-sandbox/blob/main/docs/security/threat_model.md)도 gVisor/Kata 같은 런타임, RBAC, 네트워크 격리를 별도로 요구한다. 2026-09-28 평가 기준 릴리스는 v1.0.4다. 실제 도입 시 컨트롤러와 실행 이미지는 검증한 digest로 고정한다.

샌드박스는 기본적으로 외부망과 내부망으로 나가는 연결을 모두 차단한다. 실행 서비스에서 `sandboxd`로 가는 지정 포트만 허용한다. 이미지 내부에는 사전 검토한 Python/CLI 도구를 넣고 실행 중 패키지 설치나 사용자가 지정한 임의 이미지는 허용하지 않는다. `sandboxd`와 라우터를 인터넷에 노출하지 않는다. Agent Sandbox의 기본 관리형 NetworkPolicy는 공용 인터넷 egress를 허용하므로 그대로 사용하지 않는다. [Runtime API 안내](https://agent-sandbox.sigs.k8s.io/docs/api/runtime/)의 `networkPolicyManagement: Unmanaged`와 별도 정책을 사용하려면, 실제 릴리스에서 정책 생성·선택·집행을 검증해야 한다. Kubernetes 정책은 [허용 규칙이 합쳐지는 방식](https://kubernetes.io/docs/concepts/services-networking/network-policies/)이므로 추가 deny 정책만으로 기존 allow를 취소할 수 없다.

각 실행은 제한된 시간·CPU·메모리·프로세스·임시 디스크·출력 크기를 갖고, 권한 상승·host namespace/hostPath·서비스 계정 토큰을 사용할 수 없다. 실행 종료 후 Pod와 임시 파일을 삭제한다. 로그와 결과는 비밀정보로 취급해 저장 기간을 제한하고, 코드 전문과 자격 증명을 운영 로그에 남기지 않는다. 실행기 이미지·기본 패키지는 보안 스캔과 서명/출처 검증을 거쳐 갱신한다.

실행 원장은 `queued → provisioning → running → succeeded | failed | timed_out | cancelled | outcome_unknown`을 기록한다. 동일한 `run_id`의 중복 전달은 새 Pod를 만들지 않고 기존 실행을 조회한다. 명령 전송 후 응답이 끊기면 먼저 실행 상태와 결과를 조회한다. 그래도 실행 여부를 확정할 수 없으면 자동 재실행하지 않고 `outcome_unknown`으로 보존한다. 사용자가 명시적으로 다시 요청하면 새 `run_id`를 만든다. 제어 서비스는 만료된 리소스를 청소하되 명령을 재전송하지 않는다. RelayGate는 이 흐름의 스케줄러·실행기·원장이 아니며, 향후 네트워크 경계 때문에 전송 통로가 필요할 때만 별도로 검토한다.

## 출시 조건

1. 별도 환경에서 ARM64/containerd/gVisor와 Agent Sandbox의 호환성, 실행 지연, 이미지·패키지 호환성을 측정한다. `RuntimeClass`가 실제로 gVisor를 선택하는지 Pod 내부와 노드 양쪽에서 확인한다.
2. 기본 egress 차단, DNS·클러스터 API·메타데이터·사설망·노드 로컬 서비스·다른 샌드박스 접근 차단을 **실제 Pod에서** 테스트한다. Kubernetes NetworkPolicy만으로 막히지 않는 호스트 경로는 CNI의 호스트 방화벽 등으로 보완한다. 정책을 집행하지 못하면 출시하지 않는다.
3. 서비스 계정 토큰·운영 secret·hostPath가 없고, 비특권 사용자·자원 제한·만료 삭제가 강제되는지 검증한다. 템플릿을 사용자/Agent가 수정할 수 없도록 RBAC와 admission을 검증한다.
4. 사용자와 Agent 각각의 권한 부여·취소, 소유자별 동시 실행/사용량 제한, 입력 스냅샷, 감사 기록을 검증한다. 암호화 콘텐츠가 묵시적으로 반입되지 않아야 한다.
5. timeout, 취소, 실행 서비스 재시작, 명령 응답 유실, 중복 요청 시 실행이 무단으로 반복되지 않고 결과가 정확히 분류되는지 확인한다.
6. 검증된 런타임·정책·이미지를 GitOps로 고정하고 단계적으로 활성화한다. 실행 서비스와 샌드박스 클러스터의 장애는 기존 NoteGate 읽기·쓰기 경로를 중단시키지 않아야 한다.

## 대안과 범위

- 기존 NoteGate worker/Pod에서 직접 실행: 운영 자격 증명과 런타임을 공유하므로 채택하지 않는다.
- 현재 OKE 클러스터에 gVisor/CNI를 즉시 추가: 현재 CRI-O 버전과 Flannel 상태에서 운영 변경 위험이 크므로 채택하지 않는다.
- 자체 오퍼레이터: 생명주기 요구가 기존 Agent Sandbox에서 해결되지 않는다는 검증된 근거가 생기기 전에는 만들지 않는다.
- Kubernetes Job: 단발 실행에는 더 단순할 수 있다. Agent Sandbox의 입력 전송·프로세스 실행·수명 관리가 실제로 이점을 제공하지 않거나 운영 검증에 실패하면 Job 기반 실행을 재평가한다.
- 관리형 샌드박스: 데이터 반출 정책이 결정되고 기능·비용·격리·감사 기준을 충족하면 별도 비교한다. 이 ADR은 사용을 승인하지 않는다.

이 결정은 목표 구조다. 기존 `docs/spec`의 현재 API 계약이나 배포 상태를 변경하지 않으며, 실행 API·DB 스키마·GitOps 리소스는 출시 조건과 데이터 보관 정책을 확정한 후 별도 변경으로 도입한다.
