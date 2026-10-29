#!/usr/bin/env python3
"""Publish only the verified outputs of this distribution workflow."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener, urlopen

from release_gate import validate_source


REPOSITORY = "cagojeiger/notegate"
SOURCE_REPOSITORY = "cagojeiger/notegate-core"
IMAGE = "ghcr.io/cagojeiger/notegate"
TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
)
ARCHITECTURES = ("amd64", "arm64")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
SHA = re.compile(r"[0-9a-f]{40}")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def regular_file(path):
    require(path.is_file() and not path.is_symlink(), f"Expected regular file: {path.name}")
    require(path.stat().st_size > 0, f"Empty file: {path.name}")
    return path


def require_files(directory, names):
    require(directory.is_dir() and not directory.is_symlink(), f"Missing directory: {directory}")
    require({p.name for p in directory.iterdir()} == set(names), f"Unexpected files in {directory}")
    for name in names:
        regular_file(directory / name)


def file_sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def validate_scan(report, expected_ref):
    require(isinstance(report, dict), "Invalid Trivy report")
    require(report.get("ArtifactName") == expected_ref, "Trivy report belongs to another image")
    require(report.get("ArtifactType") == "container_image", "Expected a container image scan")
    require(report.get("SchemaVersion") == 2, "Unexpected Trivy report schema")
    results = report.get("Results", [])
    require(isinstance(results, list), "Invalid Trivy results")
    for result in results:
        require(isinstance(result, dict), "Invalid Trivy result")
        vulnerabilities = result.get("Vulnerabilities", []) or []
        require(isinstance(vulnerabilities, list), "Invalid Trivy vulnerabilities")
        for vulnerability in vulnerabilities:
            require(isinstance(vulnerability, dict), "Invalid vulnerability")
            require(vulnerability.get("Severity") in ("HIGH", "CRITICAL"), "Unexpected scan severity")
            require(
                not (vulnerability["Severity"] == "CRITICAL" and vulnerability.get("FixedVersion")),
                "Fixable CRITICAL vulnerability in release scan",
            )


def prepare_assets(inputs, installer, output, version):
    cli_names = [name for target in TARGETS for name in (f"notegate-cli-{target}", f"notegate-cli-{target}.sha256")]
    require_files(inputs / "cli", cli_names)
    require_files(inputs / "digests", ARCHITECTURES)
    require_files(inputs / "scans", [f"trivy-linux-{arch}.{kind}" for arch in ARCHITECTURES for kind in ("json", "sarif")])
    regular_file(installer)
    digests = {}
    for arch in ARCHITECTURES:
        digest = (inputs / "digests" / arch).read_text(encoding="utf-8").strip()
        require(DIGEST.fullmatch(digest), f"Invalid {arch} digest")
        digests[arch] = digest
        report_path = inputs / "scans" / f"trivy-linux-{arch}.json"
        validate_scan(json.loads(report_path.read_text(encoding="utf-8")), f"{IMAGE}@{digest}")
        shutil.copyfile(report_path, output / report_path.name)
    assets = {}
    for target in TARGETS:
        name = f"notegate-cli-{target}"
        source = inputs / "cli" / name
        checksum = (inputs / "cli" / f"{name}.sha256").read_text(encoding="utf-8").strip()
        require(re.fullmatch(r"[0-9a-f]{64}", checksum), f"Invalid checksum: {name}")
        require(file_sha(source) == checksum, f"Checksum mismatch: {name}")
        shutil.copyfile(source, output / name)
        shutil.copyfile(inputs / "cli" / f"{name}.sha256", output / f"{name}.sha256")
        assets[target] = {"name": name, "sha256": checksum, "size": source.stat().st_size}
    write_json(output / "notegate-cli-manifest.json", {
        "schema_version": 1, "version": version, "repository": REPOSITORY, "assets": assets,
    })
    shutil.copyfile(installer, output / "notegate-cli-installer.sh")
    return digests


def linux_child(manifest, digest, architecture):
    if "manifests" not in manifest:
        require("config" in manifest and "layers" in manifest, "Expected an image manifest")
        return digest
    children = [entry.get("digest") for entry in manifest["manifests"]
                if entry.get("platform", {}).get("os") == "linux"
                and entry.get("platform", {}).get("architecture") == architecture]
    require(len(children) == 1 and DIGEST.fullmatch(children[0] or ""), f"Expected exactly one linux/{architecture} image")
    return children[0]


def validate_draft_assets(assets, verified):
    require(isinstance(assets, list) and len(assets) == len(verified)
            and {asset["name"] for asset in assets} == set(verified), "Draft asset set changed before publication")
    for asset in assets:
        expected = verified[asset["name"]]
        require(asset.get("id") == expected["id"] and asset.get("size") == expected["size"]
                and asset.get("state") == "uploaded", "Verified draft asset was replaced or changed")
        require(asset.get("digest") in (None, "sha256:" + expected["sha256"]), "Draft asset digest changed")


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, new_url):
        return None


class GitHub:
    def __init__(self, token):
        self.token = token

    def request(self, method, path, data=None, missing=False, upload=False):
        host = "uploads.github.com" if upload else "api.github.com"
        headers = {"Authorization": f"Bearer {self.token}", "Accept": "application/vnd.github+json",
                   "X-GitHub-Api-Version": "2022-11-28"}
        if data is not None:
            if upload:
                headers["Content-Type"] = "application/octet-stream"
            else:
                data = json.dumps(data).encode()
                headers["Content-Type"] = "application/json"
        request = Request(f"https://{host}/repos/{REPOSITORY}/{path}", data=data, headers=headers, method=method)
        try:
            with urlopen(request, timeout=120) as response:
                return json.load(response)
        except HTTPError as error:
            if missing and error.code == 404:
                return None
            raise RuntimeError(f"GitHub {method} {path.split('?')[0]} failed: HTTP {error.code}") from None

    def require_absent(self, tag):
        require(self.request("GET", f"releases/tags/{tag}", missing=True) is None, "Release already exists; refusing to overwrite")
        require(self.request("GET", f"git/ref/tags/{tag}", missing=True) is None, "Distribution tag already exists; refusing to overwrite")
        page = 1
        while True:
            releases = self.request("GET", f"releases?per_page=100&page={page}")
            require(isinstance(releases, list), "Invalid release listing")
            require(not any(item.get("tag_name") == tag for item in releases), "Draft release already exists; refusing to duplicate")
            if len(releases) < 100:
                break
            page += 1

    def download_asset_sha(self, asset_id):
        request = Request(f"https://api.github.com/repos/{REPOSITORY}/releases/assets/{asset_id}", headers={
            "Authorization": f"Bearer {self.token}", "Accept": "application/octet-stream",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        try:
            response = build_opener(NoRedirect).open(request, timeout=120)
        except HTTPError as error:
            require(error.code == 302, f"Asset download failed: HTTP {error.code}")
            location = error.headers.get("Location", "")
            parsed = urlsplit(location)
            require(parsed.scheme == "https" and parsed.hostname == "release-assets.githubusercontent.com", "Unexpected asset download host")
            # Do not forward the GitHub token to a signed object-storage URL.
            response = urlopen(Request(location), timeout=120)
        with response:
            digest = hashlib.sha256()
            size = 0
            while chunk := response.read(1024 * 1024):
                digest.update(chunk)
                size += len(chunk)
        return digest.hexdigest(), size


class Registry:
    def __init__(self):
        query = urlencode({"service": "ghcr.io", "scope": "repository:cagojeiger/notegate:pull"})
        with urlopen(f"https://ghcr.io/token?{query}", timeout=30) as response:
            self.token = json.load(response)["token"]

    def manifest(self, reference, missing=False):
        request = Request(f"https://ghcr.io/v2/cagojeiger/notegate/manifests/{quote(reference, safe=':')}", headers={
            "Authorization": f"Bearer {self.token}",
            "Accept": ", ".join(("application/vnd.oci.image.index.v1+json", "application/vnd.oci.image.manifest.v1+json",
                                 "application/vnd.docker.distribution.manifest.list.v2+json", "application/vnd.docker.distribution.manifest.v2+json")),
        })
        try:
            with urlopen(request, timeout=60) as response:
                raw = response.read()
                declared = response.headers.get("Docker-Content-Digest")
        except HTTPError as error:
            if missing and error.code == 404:
                return None
            raise RuntimeError(f"Registry manifest lookup failed: HTTP {error.code}") from None
        digest = "sha256:" + hashlib.sha256(raw).hexdigest()
        require(not declared or declared == digest, "Registry manifest digest header mismatch")
        require(not reference.startswith("sha256:") or reference == digest, "Registry manifest content digest mismatch")
        return digest, json.loads(raw)


def revalidate_source(expected):
    source = validate_source(expected["source_sha"], expected["tag"], expected["ci_run_id"])
    for name, value in expected.items():
        require(source.get(name) == value, f"Source {name} changed since release validation")
    return source


def publish():
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY, "Unexpected distribution repository")
    require(os.environ.get("GITHUB_REF") == "refs/heads/main", "Release must run on distribution main")
    require(os.environ.get("PUBLISH") == "true", "Publishing was not explicitly requested")
    version = os.environ["VERSION"]
    source_tag = os.environ["SOURCE_TAG"]
    source_sha = os.environ["SOURCE_SHA"]
    distribution_sha = os.environ["GITHUB_SHA"]
    ci_run_id = os.environ["SOURCE_CI_RUN_ID"]
    tag_object_sha = os.environ["SOURCE_TAG_OBJECT_SHA"]
    notes_sha256 = os.environ["RELEASE_NOTES_SHA256"]
    require(re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version), "Invalid release version")
    require(SHA.fullmatch(source_sha) and SHA.fullmatch(distribution_sha)
            and SHA.fullmatch(tag_object_sha), "Expected full commit and tag object SHAs")
    require(re.fullmatch(r"[1-9][0-9]*", ci_run_id), "Invalid source CI run ID")
    require(re.fullmatch(r"[0-9a-f]{64}", notes_sha256), "Invalid release notes SHA256")
    tag = f"v{version}"
    require(source_tag == tag, "Source tag must match the published release version")
    expected_source = {"version": version, "tag": source_tag, "source_sha": source_sha,
                       "ci_run_id": int(ci_run_id), "tag_object_sha": tag_object_sha,
                       "notes_sha256": notes_sha256}
    source = revalidate_source(expected_source)
    require(isinstance(source.get("release_notes"), str) and source["release_notes"].strip(), "Missing approved release notes")
    expected_source["release_notes"] = source["release_notes"]
    github = GitHub(os.environ["GH_TOKEN"])
    github.require_absent(tag)
    registry = Registry()
    require(registry.manifest(version, missing=True) is None, "Image version tag already exists; refusing to overwrite")
    output = Path(tempfile.mkdtemp(prefix="notegate-release-"))
    digests = prepare_assets(Path("/tmp/release-inputs"), Path("source/deploy/cli/install.sh"), output, version)
    expected_children = {}
    for arch, digest in digests.items():
        actual_digest, manifest = registry.manifest(digest)
        expected_children[arch] = linux_child(manifest, actual_digest, arch)

    revalidate_source(expected_source)
    github.require_absent(tag)
    require(registry.manifest(version, missing=True) is None, "Image version tag appeared before publication")
    subprocess.run(["docker", "buildx", "imagetools", "create", "-t", f"{IMAGE}:{version}",
                    *[f"{IMAGE}@{digests[arch]}" for arch in ARCHITECTURES]], check=True)
    index_digest, manifest = registry.manifest(version)
    require("manifests" in manifest, "Expected a multi-platform release index")
    for arch in ARCHITECTURES:
        require(linux_child(manifest, index_digest, arch) == expected_children[arch], f"Release {arch} image differs from scanned image")
    write_json(output / "image-digests.json", {
        "schema_version": 1, "version": version, "source_sha": source_sha, "image": IMAGE,
        "index_digest": index_digest, "architectures": digests,
        "source_repository": SOURCE_REPOSITORY, "source_ci_run_id": int(ci_run_id),
        "source_tag": source_tag, "source_tag_object_sha": tag_object_sha,
        "source_notes_sha256": notes_sha256,
        "distribution_sha": distribution_sha,
    })
    names = [f"notegate-cli-{target}{suffix}" for target in TARGETS for suffix in ("", ".sha256")]
    names += ["notegate-cli-manifest.json", "notegate-cli-installer.sh", "image-digests.json",
              "trivy-linux-amd64.json", "trivy-linux-arm64.json"]
    require_files(output, names)
    notes = (f"{source['release_notes']}\n\n---\n\n"
             f"Built from [{SOURCE_REPOSITORY}@{source_sha}](https://github.com/{SOURCE_REPOSITORY}/commit/{source_sha}).\n\n"
             f"Source tag: [{source_tag}](https://github.com/{SOURCE_REPOSITORY}/tree/{source_tag}) "
             f"(annotated object `{tag_object_sha}`; notes SHA256 `{notes_sha256}`).\n\n"
             f"Source validation: https://github.com/{SOURCE_REPOSITORY}/actions/runs/{ci_run_id}\n\n"
             f"Distribution workflow: https://github.com/{REPOSITORY}/actions/runs/{os.environ['GITHUB_RUN_ID']}\n\n"
             f"Image: `{IMAGE}@{index_digest}`\n\n"
             "Includes four native CLI builds and both immutable-image Trivy reports. "
             "Fixable CRITICAL vulnerabilities block publication.\n")
    revalidate_source(expected_source)
    github.require_absent(tag)
    draft = github.request("POST", "releases", {"tag_name": tag, "target_commitish": distribution_sha,
                                                 "name": tag, "body": notes, "draft": True, "prerelease": False})
    require(draft.get("draft") is True and draft.get("tag_name") == tag, "Unexpected draft release")
    release_id = draft["id"]
    verified_assets = {}
    for name in names:
        path = output / name
        uploaded = github.request("POST", f"releases/{release_id}/assets?{urlencode({'name': name})}", path.read_bytes(), upload=True)
        require(uploaded.get("name") == name and uploaded.get("state") == "uploaded", "Asset upload did not complete")
        actual_sha, actual_size = github.download_asset_sha(uploaded["id"])
        require(actual_sha == file_sha(path) and actual_size == path.stat().st_size, f"Uploaded asset verification failed: {name}")
        verified_assets[name] = {"id": uploaded["id"], "size": actual_size, "sha256": actual_sha}

    revalidate_source(expected_source)
    current = github.request("GET", f"releases/{release_id}")
    require(current.get("draft") is True and current.get("tag_name") == tag
            and current.get("target_commitish") == distribution_sha and current.get("body") == notes,
            "Draft release changed before publication")
    assets = github.request("GET", f"releases/{release_id}/assets?per_page=100")
    validate_draft_assets(assets, verified_assets)
    require(registry.manifest(version)[0] == index_digest, "Image version tag changed before publication")
    require(github.request("GET", f"git/ref/tags/{tag}", missing=True) is None, "Distribution tag appeared before draft publication")
    published = github.request("PATCH", f"releases/{release_id}", {"draft": False, "make_latest": "true"})
    require(published.get("draft") is False and published.get("tag_name") == tag, "Release publication failed")
    public_tag = github.request("GET", f"git/ref/tags/{tag}")
    require(public_tag["object"]["type"] == "commit" and public_tag["object"]["sha"] == distribution_sha,
            "Distribution release tag does not target the trusted distribution commit")
    revalidate_source(expected_source)
    subprocess.run(["docker", "buildx", "imagetools", "create", "-t", f"{IMAGE}:latest", f"{IMAGE}@{index_digest}"], check=True)
    require(registry.manifest("latest")[0] == index_digest, "Latest image does not match release index")
    print(f"Published {tag}: {len(names)} verified assets; image {IMAGE}@{index_digest}")


if __name__ == "__main__":
    try:
        publish()
    except (ValueError, RuntimeError, HTTPError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Release stopped: {error}") from None
