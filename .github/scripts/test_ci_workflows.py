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
                for forbidden in ('Swatinem/rust-cache', 'cache-from:', 'cache-to:', 'cache: pnpm', 'CORE_READ_APP_', 'RELEASE_APP_'):
                    self.assertNotIn(forbidden, text)
                for step in re.split(r'(?m)^      - ', text):
                    if 'uses: aquasecurity/trivy-action@' in step:
                        self.assertRegex(step, r'(?m)^          cache: false$')
                    if 'uses: docker/setup-buildx-action@' in step:
                        self.assertRegex(step, r'(?m)^          cache-binary: false$')

    def test_core_checkout_is_fixed_and_anonymous(self):
        action = (ROOT / '.github/actions/checkout-core/action.yml').read_text()
        self.assertIn('https://github.com/cagojeiger/notegate-core.git', action)
        self.assertIn('credential.helper= -c http.extraHeader=', action)
        self.assertNotIn('token:', action)
        self.assertIn('rev-parse HEAD', action)

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
