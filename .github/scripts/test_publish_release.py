import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError
from urllib.parse import parse_qs

import publish_release as release


class AssetFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.inputs = self.root / "inputs"
        self.output = self.root / "output"
        self.output.mkdir()
        for name in ("cli", "digests", "scans"):
            (self.inputs / name).mkdir(parents=True)
        self.digests = {arch: "sha256:" + digit * 64 for arch, digit in (("amd64", "a"), ("arm64", "b"))}
        for arch, digest in self.digests.items():
            (self.inputs / "digests" / arch).write_text(digest + "\n")
            report = {"SchemaVersion": 2, "ArtifactType": "container_image",
                      "ArtifactName": f"{release.IMAGE}@{digest}", "Results": []}
            (self.inputs / "scans" / f"trivy-linux-{arch}.json").write_text(json.dumps(report))
            (self.inputs / "scans" / f"trivy-linux-{arch}.sarif").write_text("{}")
        for target in release.TARGETS:
            binary = ("fixture CLI " + target).encode()
            name = f"notegate-cli-{target}"
            (self.inputs / "cli" / name).write_bytes(binary)
            (self.inputs / "cli" / f"{name}.sha256").write_text(hashlib.sha256(binary).hexdigest() + "\n")
        self.installer = self.root / "install.sh"
        self.installer.write_text("#!/bin/sh\nexit 0\n")

    def prepare(self):
        return release.prepare_assets(self.inputs, self.installer, self.output, "0.1.120")


class ReleaseAssetTests(AssetFixture):
    def test_preserves_existing_cli_manifest_contract_and_approved_asset_set(self):
        self.assertEqual(self.prepare(), self.digests)
        manifest = json.loads((self.output / "notegate-cli-manifest.json").read_text())
        self.assertEqual(set(manifest), {"schema_version", "version", "repository", "assets"})
        self.assertEqual(manifest["repository"], "cagojeiger/notegate")
        self.assertEqual(manifest["version"], "0.1.120")
        self.assertEqual(set(manifest["assets"]), set(release.TARGETS))
        self.assertEqual(len(list(self.output.iterdir())), 12)  # Image provenance is added after registry verification.
        for target, asset in manifest["assets"].items():
            self.assertEqual(set(asset), {"name", "sha256", "size"})
            self.assertEqual(asset["name"], f"notegate-cli-{target}")
            self.assertEqual(asset["sha256"], release.file_sha(self.output / asset["name"]))
            self.assertEqual(asset["size"], (self.output / asset["name"]).stat().st_size)
        self.assertFalse(list(self.output.glob("*.sarif")))

    def test_rejects_modified_cli_without_matching_sidecar(self):
        (self.inputs / "cli" / f"notegate-cli-{release.TARGETS[0]}").write_bytes(b"modified")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            self.prepare()

    def test_rejects_missing_cli_platform(self):
        (self.inputs / "cli" / f"notegate-cli-{release.TARGETS[-1]}").unlink()
        with self.assertRaisesRegex(ValueError, "Unexpected files"):
            self.prepare()

    def test_rejects_extra_downloaded_files(self):
        (self.inputs / "cli" / "source.tar.gz").write_bytes(b"must not publish")
        with self.assertRaisesRegex(ValueError, "Unexpected files"):
            self.prepare()

    def test_rejects_symlink_input(self):
        binary = self.inputs / "cli" / f"notegate-cli-{release.TARGETS[0]}"
        binary.unlink()
        binary.symlink_to(self.installer)
        with self.assertRaisesRegex(ValueError, "Expected regular file"):
            self.prepare()

    def test_rejects_report_for_another_digest(self):
        report = self.inputs / "scans" / "trivy-linux-arm64.json"
        value = json.loads(report.read_text())
        value["ArtifactName"] = f"{release.IMAGE}@{self.digests['amd64']}"
        report.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "another image"):
            self.prepare()

    def test_rejects_fixable_critical_but_keeps_unfixed_evidence(self):
        report = {"SchemaVersion": 2, "ArtifactType": "container_image", "ArtifactName": "expected", "Results": [
            {"Vulnerabilities": [{"Severity": "HIGH", "FixedVersion": "1.2.3"}, {"Severity": "CRITICAL"}]}]}
        release.validate_scan(report, "expected")
        report["Results"][0]["Vulnerabilities"][-1]["FixedVersion"] = "2.0.0"
        with self.assertRaisesRegex(ValueError, "Fixable CRITICAL"):
            release.validate_scan(report, "expected")


class RegistryAndPublicationTests(unittest.TestCase):
    def test_verified_draft_asset_cannot_be_replaced_under_same_name(self):
        checksum = "a" * 64
        verified = {"cli": {"id": 7, "size": 3, "sha256": checksum}}
        asset = {"id": 7, "name": "cli", "size": 3, "state": "uploaded", "digest": "sha256:" + checksum}
        release.validate_draft_assets([asset], verified)
        with self.assertRaisesRegex(ValueError, "replaced or changed"):
            release.validate_draft_assets([{**asset, "id": 8}], verified)
        with self.assertRaisesRegex(ValueError, "digest changed"):
            release.validate_draft_assets([{**asset, "digest": "sha256:" + "b" * 64}], verified)

    def test_selects_only_matching_linux_child_and_rejects_ambiguity(self):
        digest = "sha256:" + "a" * 64
        child = {"digest": digest, "platform": {"os": "linux", "architecture": "amd64"}}
        manifest = {"manifests": [child, {"digest": "sha256:" + "b" * 64, "platform": {"os": "unknown", "architecture": "unknown"}}]}
        self.assertEqual(release.linux_child(manifest, "unused", "amd64"), digest)
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.linux_child(manifest, "unused", "arm64")
        manifest["manifests"].append(child)
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.linux_child(manifest, "unused", "amd64")

    def test_release_duplicate_and_existing_tag_are_never_overwritten(self):
        github = release.GitHub("fixture")
        with patch.object(github, "request", return_value={"id": 1}) as request:
            with self.assertRaisesRegex(ValueError, "Release already exists"):
                github.require_absent("v0.1.120")
            self.assertEqual(request.call_count, 1)
        with patch.object(github, "request", side_effect=[None, {"ref": "refs/tags/v0.1.120"}]):
            with self.assertRaisesRegex(ValueError, "tag already exists"):
                github.require_absent("v0.1.120")

    def test_untagged_draft_blocks_duplicate_publication(self):
        github = release.GitHub("fixture")
        with patch.object(github, "request", side_effect=[None, None, [{"tag_name": "v0.1.120", "draft": True}]]):
            with self.assertRaisesRegex(ValueError, "Draft release already exists"):
                github.require_absent("v0.1.120")

    def test_checks_all_release_pages_for_matching_draft(self):
        github = release.GitHub("fixture")
        first_page = [{"tag_name": f"v0.0.{index}"} for index in range(100)]
        with patch.object(github, "request", side_effect=[None, None, first_page, [{"tag_name": "v0.1.120", "draft": True}]]):
            with self.assertRaisesRegex(ValueError, "Draft release already exists"):
                github.require_absent("v0.1.120")

    def test_authentication_error_is_not_treated_as_missing_release(self):
        github = release.GitHub("fixture")
        with patch.object(release, "urlopen", side_effect=HTTPError("https://api.github.com", 403, "Forbidden", {}, None)):
            with self.assertRaisesRegex(RuntimeError, "HTTP 403"):
                github.request("GET", "releases/tags/v0.1.120", missing=True)

    def test_plan_only_and_non_main_cannot_publish(self):
        base = {"GITHUB_REPOSITORY": release.REPOSITORY, "GITHUB_REF": "refs/heads/main", "PUBLISH": "false"}
        with patch.dict(release.os.environ, base, clear=True):
            with self.assertRaisesRegex(ValueError, "explicitly requested"):
                release.publish()
        base.update(PUBLISH="true", GITHUB_REF="refs/heads/test")
        with patch.dict(release.os.environ, base, clear=True):
            with self.assertRaisesRegex(ValueError, "distribution main"):
                release.publish()


class TaggedPublicationFlowTests(AssetFixture):
    def setUp(self):
        super().setUp()
        self.source_sha = "1" * 40
        self.distribution_sha = "2" * 40
        self.tag_object_sha = "3" * 40
        self.notes = "Fix terminal sign-in and preserve existing sessions."
        self.notes_sha = hashlib.sha256(self.notes.encode()).hexdigest()
        self.source = {"version": "0.1.120", "tag": "v0.1.120", "source_sha": self.source_sha,
                       "ci_run_id": 12345, "tag_object_sha": self.tag_object_sha,
                       "notes_sha256": self.notes_sha, "release_notes": self.notes}
        self.environment = {
            "GITHUB_REPOSITORY": release.REPOSITORY, "GITHUB_REF": "refs/heads/main",
            "GITHUB_SHA": self.distribution_sha, "GITHUB_RUN_ID": "54321",
            "PUBLISH": "true", "VERSION": "0.1.120", "SOURCE_SHA": self.source_sha,
            "SOURCE_TAG": "v0.1.120", "SOURCE_CI_RUN_ID": "12345",
            "SOURCE_TAG_OBJECT_SHA": self.tag_object_sha, "RELEASE_NOTES_SHA256": self.notes_sha,
            "GH_TOKEN": "distribution-token", "CORE_TOKEN": "source-read-token",
        }
        self.github = Mock()
        self.github.request.side_effect = self.github_request
        self.github.download_asset_sha.side_effect = lambda asset_id: self.upload_hashes[asset_id]
        self.registry = Mock()
        self.registry.manifest.side_effect = self.registry_manifest
        self.commands = []
        self.image_tag_created = False
        self.latest_created = False
        self.published = False
        self.draft = None
        self.uploads = {}
        self.upload_hashes = {}
        self.children = {"amd64": "sha256:" + "c" * 64, "arm64": "sha256:" + "d" * 64}
        self.index_digest = "sha256:" + "e" * 64
        self.index = {"manifests": [self.child(arch) for arch in release.ARCHITECTURES]}

    def child(self, architecture):
        return {"digest": self.children[architecture], "platform": {"os": "linux", "architecture": architecture}}

    def registry_manifest(self, reference, missing=False):
        for architecture, digest in self.digests.items():
            if reference == digest:
                return digest, {"manifests": [self.child(architecture)]}
        if reference == "0.1.120":
            if not self.image_tag_created:
                self.assertTrue(missing)
                return None
            return self.index_digest, self.index
        if reference == "latest":
            self.assertTrue(self.latest_created)
            return self.index_digest, self.index
        self.fail(f"Unexpected registry lookup: {reference}")

    def docker(self, command, check):
        self.assertTrue(check)
        self.assertEqual(command[:4], ["docker", "buildx", "imagetools", "create"])
        self.commands.append(command)
        if f"{release.IMAGE}:0.1.120" in command:
            self.assertEqual(command[-2:], [f"{release.IMAGE}@{self.digests[arch]}" for arch in release.ARCHITECTURES])
            self.image_tag_created = True
        elif f"{release.IMAGE}:latest" in command:
            self.assertTrue(self.published)
            self.assertEqual(command[-1], f"{release.IMAGE}@{self.index_digest}")
            self.latest_created = True
        else:
            self.fail("Unexpected Docker write")

    def github_request(self, method, path, data=None, missing=False, upload=False):
        if method == "POST" and path == "releases":
            self.assertFalse(self.published)
            self.assertEqual(data["target_commitish"], self.distribution_sha)
            self.assertEqual(data["tag_name"], "v0.1.120")
            self.assertTrue(data["draft"])
            self.draft = {**data, "id": 10}
            return self.draft.copy()
        if method == "POST" and path.startswith("releases/10/assets?"):
            self.assertTrue(upload)
            name = parse_qs(path.partition("?")[2])["name"][0]
            asset_id = len(self.uploads) + 1
            sha = hashlib.sha256(data).hexdigest()
            asset = {"id": asset_id, "name": name, "size": len(data), "state": "uploaded", "digest": "sha256:" + sha}
            self.uploads[name] = {"metadata": asset, "data": data}
            self.upload_hashes[asset_id] = sha, len(data)
            return asset.copy()
        if method == "GET" and path == "releases/10":
            return self.draft.copy()
        if method == "GET" and path == "releases/10/assets?per_page=100":
            return [asset["metadata"].copy() for asset in self.uploads.values()]
        if method == "PATCH" and path == "releases/10":
            self.assertEqual(len(self.uploads), 13)
            self.assertEqual(data, {"draft": False, "make_latest": "true"})
            self.published = True
            self.draft["draft"] = False
            return self.draft.copy()
        if method == "GET" and path == "git/ref/tags/v0.1.120":
            if self.published:
                return {"object": {"type": "commit", "sha": self.distribution_sha}}
            self.assertTrue(missing)
            return None
        self.fail(f"Unexpected GitHub operation: {method} {path}")

    def run_publish(self, validations=None):
        prepare = release.prepare_assets
        with patch.dict(release.os.environ, self.environment, clear=True), \
                patch.object(release, "GitHub", return_value=self.github) as github, \
                patch.object(release, "Registry", return_value=self.registry), \
                patch.object(release, "validate_source", side_effect=validations or (lambda *args: self.source.copy())) as validate, \
                patch.object(release.tempfile, "mkdtemp", return_value=str(self.output)), \
                patch.object(release, "prepare_assets", side_effect=lambda inputs, installer, output, version: prepare(self.inputs, self.installer, output, version)), \
                patch.object(release.subprocess, "run", side_effect=self.docker):
            release.publish()
            github.assert_called_once_with("distribution-token")
        return validate

    def test_full_publication_freezes_approved_tag_notes_and_provenance(self):
        validate = self.run_publish()
        self.assertTrue(self.published)
        self.assertTrue(self.latest_created)
        self.assertEqual(len(self.commands), 2)
        self.assertEqual(validate.call_count, 5)
        for call in validate.call_args_list:
            self.assertEqual(call.args, (self.source_sha, "v0.1.120", 12345))
        self.assertTrue(self.draft["body"].startswith(self.notes + "\n\n---\n\n"))
        self.assertIn(self.tag_object_sha, self.draft["body"])
        self.assertIn(self.notes_sha, self.draft["body"])
        provenance = json.loads(self.uploads["image-digests.json"]["data"])
        self.assertEqual(provenance["source_sha"], self.source_sha)
        self.assertEqual(provenance["source_tag"], "v0.1.120")
        self.assertEqual(provenance["source_tag_object_sha"], self.tag_object_sha)
        self.assertEqual(provenance["source_notes_sha256"], self.notes_sha)
        self.assertEqual(provenance["source_ci_run_id"], 12345)
        self.assertEqual(provenance["distribution_sha"], self.distribution_sha)
        self.assertEqual(provenance["index_digest"], self.index_digest)

    def test_moved_tag_aborts_before_first_publication_write(self):
        moved = {**self.source, "tag_object_sha": "4" * 40}
        with self.assertRaisesRegex(ValueError, "tag_object_sha changed"):
            self.run_publish([self.source.copy(), moved])
        self.assertFalse(self.commands)
        self.assertFalse(self.github.request.called)
        self.assertFalse(self.published)

    def test_moved_tag_after_asset_upload_keeps_draft_unpublished(self):
        moved = {**self.source, "tag_object_sha": "4" * 40}
        with self.assertRaisesRegex(ValueError, "tag_object_sha changed"):
            self.run_publish([self.source.copy(), self.source.copy(), self.source.copy(), moved])
        self.assertTrue(self.image_tag_created)
        self.assertEqual(len(self.uploads), 13)
        self.assertTrue(self.draft["draft"])
        self.assertFalse(self.published)
        self.assertFalse(self.latest_created)


if __name__ == "__main__":
    unittest.main()
