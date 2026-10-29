# NoteGate

Public distribution repository for NoteGate.

Source code, development, issues, and pull requests: [notegate-core](https://github.com/cagojeiger/notegate-core).

## CLI

```sh
curl -fsSL https://github.com/cagojeiger/notegate/releases/latest/download/notegate-cli-installer.sh | sh
```

## Container

```sh
docker pull ghcr.io/cagojeiger/notegate:0.1.119
```

The v0.1.119 release assets are mirrored without modification from the original release. Application source for this version is available at [notegate-core/v0.1.119](https://github.com/cagojeiger/notegate-core/tree/v0.1.119).

[Earlier releases](https://github.com/cagojeiger/notegate-core/releases) remain in notegate-core. Source archives automatically generated for this distribution repository contain distribution metadata, not the application source.

## Repository layout

```text
LICENSE                      Existing AGPL license, unchanged
README.md                    Distribution and release instructions
.github/workflows/           CI execution, release, and workflow validation
.github/scripts/             Source gates, publication, and their tests
```

Application code, Dockerfiles, and development documentation stay in
`notegate-core`. CI and release builds check out an exact core commit into a temporary `source/`
directory on the runner. This directory is ignored and must not enter this
repository's history. Public policy files may be added after review.

## Release

Merge a release PR in core with the version change and exactly one `## Release notes`
section containing only approved public release notes, ending at a standalone
`<!-- release-notes:end -->` line. A missing boundary stops publication. After that commit's main
CI succeeds, the core dispatcher creates an annotated `vMAJOR.MINOR.PATCH` source
tag and dispatches **Release** here with the tag, its exact source SHA, and the CI
run ID. The annotated tag message holds the approved release notes. Build and
publication workflows are maintained only in this distribution repository.

Manual dispatch from this repository's `main` takes the same `source_tag`,
`source_sha`, and `source_ci_run_id` inputs. `publish=false` validates only the
plan; it does not build, push an image, or publish a release. `publish=true` builds
both Linux container images and four native CLI binaries, scans the immutable
images, and publishes verified assets here and to `ghcr.io/cagojeiger/notegate`.

The annotated source tag must match core's VERSION and remain an ancestor of core
main. Newer main commits do not change the selected source. The supplied CI must
be the latest run for that exact commit and have successful required jobs,
including RustSec, Rust, PostgreSQL, Web, and Browser E2E. Later checks reject a
moved tag or changed release notes. The version must be newer than the latest
public release and unused by a distribution tag, draft, release, or image tag.
Fixable CRITICAL image vulnerabilities block publication; HIGH and unfixed
CRITICAL findings remain visible in the published reports.

Release notes and `image-digests.json` identify the real core tag, source SHA, tag
object, approved notes digest, and CI run. Distribution release tags point to the
workflow commit in this repository; they do not identify application source.
CLI asset names, manifest schema, installer URLs, and the container address stay
unchanged. A stopped publication may leave an image tag or draft; diagnose that
state before retrying. The workflow never overwrites or deletes a release.

Before the first publication, grant this repository **Write** access under the
existing GHCR package's **Manage Actions access**. Replace the old core publishing
workflow with the tag-and-dispatch controller before enabling it; only this
repository publishes images and release assets. Core retains the CI dispatcher and
status checks; execution runs here. Post-release browser/text performance workflows
are not automatically connected.

## CI execution

Core dispatches CI, security scans, and manual performance measurements to this
repository for a validated core run and exact commit. Core request/result jobs use
the repository-scoped `notegate-core-wait` self-hosted runner scale set; dispatch
and source-tag recording use `notegate-core-control`. Keeping dispatch on a separate
scale set prevents waiting jobs from occupying every dispatcher runner. Long builds,
tests, scans and publication use standard public GitHub-hosted runners here.
Dependabot update jobs remain in core. Public workers do not use Actions caches.
Browser performance can reuse a validated public CI frontend artifact or build one
for the selected commit.

PR execution is restricted to branches in core authored by the repository owner
or the official Dependabot account. Forks and other authors fail closed. This is
a trusted-source policy, not an untrusted-contributor sandbox. The gate validates
the current PR head, base, synthetic merge, source run and attempt before checkout.

Source Security runs Semgrep CE with pinned local Python/JavaScript/TypeScript
security-audit rules and a Gitleaks current-tree scan. Only aggregate outcomes and
rule identifiers are public; raw scanner output is not uploaded. RustSec, npm audit,
Trivy and isolated DAST remain separate checks. These scanners do not reproduce
CodeQL's analysis or GitHub Dependency Review's dependency-diff interface and policy.
Native Dependency Review runs only while core is public. See
[scanner rule sources](.github/security-rules/README.md) for coverage and licensing.

## Source access

The dispatcher in core uses `RELEASE_APP_CLIENT_ID` (Actions variable) and
`RELEASE_APP_PRIVATE_KEY` (Actions secret) to request an App token restricted to
this distribution repository with Actions write permission. This is separate from
core's ordinary `GITHUB_TOKEN`, which creates the source tag in core.

For private core checkout and CI verification, configure `CORE_READ_APP_CLIENT_ID`
(Actions variable) and `CORE_READ_APP_PRIVATE_KEY` (Actions secret) here. The App
installation must include only `notegate-core` with Contents read, Actions read and
Pull requests read permissions. Metadata gates request those read permissions;
each source checkout requests only Contents read. Checkout credentials are not
persisted, and the checkout token must be revoked successfully before source
execution. A checkout or revocation failure stops the job. Distribution publication uses this
repository's `GITHUB_TOKEN` for Releases and GHCR, not the core read token.

The App private key is supplied to the same job that later executes trusted source.
Revoking the installation token does not isolate or revoke that long-lived key.
Compromised dependencies or source with runner access remain a credential risk;
this workflow must not execute untrusted PRs. The App has no core write permission.
Runner-registration credentials are separate and must never be supplied here.

The configured read App is required even while core is public. Validation of this
distribution repository uses synthetic tests without App credentials or application
checkout. Synthetic tests do not prove private access: qualify exact-SHA PR/main,
failure and release paths before changing core visibility. Private personal core
does not require enforced merge protection under this deployment policy.

Both repositories currently remain public. No visibility change is performed by
these workflows. Before making core private, review public build logs, artifacts,
and caches separately; private Git history alone does not make public build
outputs confidential. The AGPL license remains unchanged.
