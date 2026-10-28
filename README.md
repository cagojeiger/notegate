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
.github/workflows/           Release and workflow validation
.github/scripts/             Release gates, publication, and their tests
```

Application code, Dockerfiles, development documentation, and ordinary CI stay in
`notegate-core`. Builds check out an exact core commit into a temporary `source/`
directory on the runner. This directory is ignored and must not enter this
repository's history. Public policy files may be added after review.

## Release

Run **Release** from this repository's `main`, supplying the full current core
`main` SHA. `publish=false` only validates the plan; it does not build, push an
image, or publish a release. `publish=true` builds both Linux container images and
four native CLI binaries, scans the immutable images, and publishes verified
assets to this repository and `ghcr.io/cagojeiger/notegate`.

The current core commit must have successful main CI, including RustSec, Rust,
PostgreSQL, Web, and Browser E2E. The version must be newer than the latest public
release and unused by a distribution tag, draft, release, or image version tag.
Fixable CRITICAL image vulnerabilities block publication; HIGH and unfixed
CRITICAL findings remain visible in the published reports.

Release notes and `image-digests.json` identify the real core source SHA and CI
run. Distribution release tags point to the workflow commit in this repository.
CLI asset names, manifest schema, installer URLs, and the container address stay
unchanged. A stopped publication may leave an image tag or draft; diagnose that
state before retrying. The workflow never overwrites or deletes a release.

Before the first publication, grant this repository **Write** access under the
existing GHCR package's **Manage Actions access**. Keep the core Release workflow
disabled to avoid two publishers. Core CI and post-release browser/text performance
workflows have not been migrated or connected to this workflow yet.

Both repositories currently remain public. This first release migration requires
public core access and stops if that changes. Private source access and a separate
review of public logs, artifacts, and caches are required before changing visibility.
