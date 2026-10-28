import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import publish_release as release


class ReleaseAssetTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
