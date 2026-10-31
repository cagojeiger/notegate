"""Static checks for the public CI trust and execution boundary."""
import importlib.util
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / '.github' / 'workflows'
KINDS = ('ci', 'dependency-security', 'image-security', 'security-dast',
         'browser-performance', 'text-write-performance')


class WorkflowBoundaryTests(unittest.TestCase):
    def test_api_error_diagnostics_are_read_from_the_written_paths(self):
        text = (WORKFLOWS / 'ci.yml').read_text()
        job = text.split('  browser-api:\n', 1)[1].split('\n  browser:\n', 1)[0]
        self.assertIn('        working-directory: source\n', job)
        step = job.split('      - name: Test public V2 API\n', 1)[1].split('\n      - name:', 1)[0]
        for option, suffix in (('--output', 'json'), ('--dump-header', 'headers')):
            path = f'browser-e2e/public-v2-error.{suffix}'
            self.assertIn(f'{option} {path}', step)
            self.assertEqual(step.count(path), 2)
            self.assertNotIn(f'source/{path}', step)
        self.assertRegex(step, r'(?s)jq --exit-status.*?browser-e2e/public-v2-error\.json')
        self.assertRegex(step, r'(?s)grep --ignore-case --quiet.*?browser-e2e/public-v2-error\.headers')

    def test_run_steps_do_not_enter_source_before_checkout(self):
        for kind in KINDS:
            text = (WORKFLOWS / f'{kind}.yml').read_text()
            for job in re.split(r'(?m)^  [a-z][a-z0-9-]*:\n', text)[1:]:
                if '        working-directory: source\n' not in job.split('    steps:\n', 1)[0]:
                    continue
                steps = re.split(r'(?m)^      - ', job.split('    steps:\n', 1)[1])[1:]
                checked_out = False
                for step in steps:
                    if 'uses: ./.control/.github/actions/checkout-core' in step:
                        checked_out = True
                    if not checked_out and re.search(r'(?m)^        run:', step):
                        with self.subTest(workflow=kind, step=step.splitlines()[0]):
                            self.assertIn('        working-directory: ${{ github.workspace }}\n', step)

    def test_dispatches_gate_and_avoid_caches(self):
        for kind in KINDS:
            with self.subTest(kind=kind):
                text = (WORKFLOWS / f'{kind}.yml').read_text()
                self.assertIn('  workflow_dispatch:\n    inputs:\n      request:', text)
                self.assertIn(f'run-name: Core {kind} ', text)
                self.assertIn(f'ci_bridge.py gate --kind {kind}', text)
                self.assertIn('  gate:\n    name: Validate core request\n    permissions:\n      contents: read\n      actions: read', text)
                self.assertIn('ref: ${{ github.sha }}\n          path: .control\n          persist-credentials: false', text)
                for forbidden in ('Swatinem/rust-cache', 'cache-from:', 'cache-to:', 'cache: pnpm', 'RELEASE_APP_'):
                    self.assertNotIn(forbidden, text)
                gate = text.split('      - name: Validate immutable core request', 1)[0]
                self.assertIn('permission-pull-requests: read', gate)
                self.assertIn('CORE_TOKEN: ${{ steps.core-token.outputs.token }}', text)
                for step in re.split(r'(?m)^      - ', text):
                    if 'uses: ./.control/.github/actions/checkout-core' in step:
                        self.assertIn('client-id: ${{ vars.CORE_READ_APP_CLIENT_ID }}', step)
                        self.assertIn('private-key: ${{ secrets.CORE_READ_APP_PRIVATE_KEY }}', step)
                    if 'uses: aquasecurity/trivy-action@' in step:
                        self.assertRegex(step, r'(?m)^          cache: false$')
                    if 'uses: docker/setup-buildx-action@' in step:
                        self.assertRegex(step, r'(?m)^          cache-binary: false$')

    def test_core_checkout_is_fixed_scoped_and_revoked_before_source_execution(self):
        action = (ROOT / '.github/actions/checkout-core/action.yml').read_text()
        self.assertIn('https://github.com/cagojeiger/notegate-core.git', action)
        self.assertIn('repositories: notegate-core', action)
        self.assertIn('permission-contents: read', action)
        self.assertNotIn('permission-actions:', action)
        self.assertIn('GIT_CONFIG_COUNT=3', action)
        self.assertIn('GIT_CONFIG_KEY_0=credential.helper', action)
        self.assertIn('GIT_CONFIG_KEY_1=http.extraHeader', action)
        self.assertIn('always() && steps.token.outputs.token', action)
        self.assertIn('https://api.github.com/installation/token', action)
        self.assertIn('test "$status" = 204', action)
        self.assertNotIn('continue-on-error', action)
        self.assertIn('rev-parse HEAD', action)

    def test_source_security_and_dependency_audits_are_not_path_or_event_optional(self):
        text = (WORKFLOWS / 'ci.yml').read_text()
        job = text.split('  security:\n', 1)[1].split('\n  hygiene:\n', 1)[0]
        self.assertIn('needs: gate', job)
        self.assertNotIn('\n    if:', job)
        self.assertIn('uses: ./.control/.github/actions/setup-security-scanners', job)
        setup = (ROOT / '.github/actions/setup-security-scanners/action.yml').read_text()
        self.assertIn('16b9e6d45f10649f6a531ec4838ed9924372c3bc', setup)
        self.assertIn('semgrep==1.182.0', setup)
        self.assertIn('sha256sum --check', setup)
        self.assertIn('security_scan.py semgrep source', job)
        self.assertIn('security_scan.py gitleaks source', job)
        self.assertNotIn('upload-artifact', job)
        dependency = (WORKFLOWS / 'dependency-security.yml').read_text()
        self.assertIn('name: Npm Audit', dependency)
        self.assertNotIn("source_event != 'pull_request'", dependency)

    def test_source_steps_capture_diagnostics_without_hiding_mask_commands(self):
        for kind in (*KINDS, 'release'):
            text = (WORKFLOWS / f'{kind}.yml').read_text()
            for job in re.split(r'(?m)^  [a-z][a-z0-9-]*:\n', text)[1:]:
                source = False
                for step in re.split(r'(?m)^      - ', job):
                    if 'uses: ./.control/.github/actions/checkout-core' in step:
                        source = True
                    if source and re.search(r'(?m)^        run:', step):
                        with self.subTest(workflow=kind, step=step.splitlines()[0]):
                            if '::add-mask::' in step:
                                self.assertIn('shell: bash\n', step)
                                self.assertIn('2>"$RUNNER_TEMP/seed-user.stderr"', step)
                            elif 'security_scan.py' in step:
                                self.assertIn('shell: bash\n', step)
                            else:
                                self.assertIn('shell: private_run.sh {0}', step)

    def test_raw_source_diagnostics_are_not_public_artifacts(self):
        ci = (WORKFLOWS / 'ci.yml').read_text()
        for forbidden in ('path: source/target/test-suites/', 'source/frontend/web/test-results\n',
                          'source/browser-e2e/*.log', 'source/browser-ui/*.log', 'docker/build-push-action@'):
            self.assertNotIn(forbidden, ci)
        release = (WORKFLOWS / 'release.yml').read_text()
        self.assertNotIn('docker/build-push-action@', release)
        self.assertIn('--metadata-file "$metadata"', release)
        self.assertIn('containerimage.digest', release)
        text = (WORKFLOWS / 'text-write-performance.yml').read_text()
        self.assertNotIn('path: source/text-write-results/\n', text)
        self.assertIn('source/text-write-results/summary.md', text)
        dast = (WORKFLOWS / 'security-dast.yml').read_text()
        self.assertNotIn('path: source/security-results/', dast)

    def test_cross_run_download_has_job_actions_permission(self):
        for kind in KINDS:
            text = (WORKFLOWS / f'{kind}.yml').read_text()
            workflow_permissions = re.search(r'(?m)^permissions:\n((?:  [^\n]+\n)+)', text)
            for job in re.split(r'(?m)^  [a-z][a-z0-9-]*:\n', text)[1:]:
                for step in re.split(r'(?m)^      - ', job):
                    if 'uses: actions/download-artifact@' in step and re.search(r'(?m)^          run-id:', step):
                        with self.subTest(workflow=kind, step=step.splitlines()[0]):
                            header = job.split('    steps:\n', 1)[0]
                            job_permissions = re.search(r'(?m)^    permissions:\n((?:      [^\n]+\n)+)', header)
                            permissions = job_permissions or workflow_permissions
                            self.assertIsNotNone(permissions)
                            self.assertRegex(permissions.group(1), r'(?m)^\s+actions: read$')
                            self.assertIn('github-token: ${{ github.token }}', step)

    def test_selection_uses_gate_and_performance_uses_verified_producer(self):
        ci = (WORKFLOWS / 'ci.yml').read_text()
        self.assertNotIn('deploy/ci/ci-impact.py', ci)
        for job in ('rust_build', 'rust', 'web', 'browser', 'postgres', 'image'):
            self.assertIn(f'needs.gate.outputs.{job}', ci)
        perf = (WORKFLOWS / 'browser-performance.yml').read_text()
        self.assertIn('needs.gate.outputs.web_artifact_run_id', perf)
        self.assertIn('path: source/frontend/web/dist', perf)
        text = (WORKFLOWS / 'text-write-performance.yml').read_text()
        self.assertIn('sha: ${{ steps.baseline.outputs.sha }}', text)
        self.assertIn('fetch-depth: 0', text)

    def test_impact_conservatively_classifies_missing_or_unknown_paths(self):
        spec = importlib.util.spec_from_file_location('ci_impact', ROOT / '.github/scripts/ci_impact.py')
        impact = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(impact)
        for event, paths in [('push', []), ('pull_request', []), ('pull_request', ['unrecognized'])]:
            result = impact.classify(event, paths)
            self.assertTrue(all(result[job] for job in impact.JOBS))
        docs = impact.classify('pull_request', ['docs/example.md'])
        self.assertFalse(any(docs[job] for job in impact.JOBS))


if __name__ == '__main__':
    unittest.main()
