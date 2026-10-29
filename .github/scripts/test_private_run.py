"""Synthetic tests for the public workflow's private-output shell adapter."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

WRAPPER = Path(__file__).with_name("private_run.sh")


class PrivateRunTests(unittest.TestCase):
    def invoke(self, script, directory, **extra):
        path = Path(directory) / "step.sh"
        path.write_text(script)
        env = {**os.environ, "RUNNER_TEMP": directory, **extra}
        return subprocess.run(["bash", str(WRAPPER), str(path)], cwd=directory, env=env,
                              text=True, capture_output=True)

    def test_success_keeps_output_file_and_working_directory_but_hides_logs(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "gha-output"
            summary = Path(directory) / "gha-summary"
            summary.write_text("existing summary\n")
            result = self.invoke('printf "sensitive stdout\\n"\nprintf "sensitive stderr\\n" >&2\n'
                                 'printf "private summary\\n" >> "$GITHUB_STEP_SUMMARY"\n'
                                 'printf "where=%s\\n" "$PWD" >> "$GITHUB_OUTPUT"\n',
                                 directory, GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(summary))
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "Private step finished (exit 0).\n")
            self.assertEqual(result.stderr, "")
            self.assertEqual(output.read_text(), f"where={Path(directory).resolve()}\n")
            self.assertEqual(summary.read_text(), "existing summary\n")
            self.assertFalse(list(Path(directory).glob("private-run.*")))

    def test_failure_and_pipefail_propagate_without_raw_output(self):
        with tempfile.TemporaryDirectory() as directory:
            for script, expected in [('printf "private error\\n" >&2\nexit 7\n', 7),
                                     ('false | true\nprintf "must not run\\n"\n', 1),
                                     ('true\nfalse\n', 1)]:
                with self.subTest(script=script):
                    result = self.invoke(script, directory)
                    self.assertEqual(result.returncode, expected)
                    self.assertEqual(result.stdout, f"Private step finished (exit {expected}).\n")
                    self.assertEqual(result.stderr, "")
                    self.assertFalse(list(Path(directory).glob("private-run.*")))

    def test_missing_input_fails_without_echoing_private_path(self):
        with tempfile.TemporaryDirectory() as directory:
            env = {**os.environ, "RUNNER_TEMP": directory}
            for arguments in ([], [str(Path(directory) / "sensitive-missing")]):
                result = subprocess.run(["bash", str(WRAPPER), *arguments], env=env, text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stderr, "Private step could not start.\n")
                self.assertEqual(result.stdout, "")
            env.pop("RUNNER_TEMP")
            result = subprocess.run(["bash", str(WRAPPER), __file__], env=env, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stderr, "Private step could not start.\n")

    def test_cancelled_step_does_not_succeed_and_cleans_output(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "step.sh"
            marker = Path(directory) / "ready"
            path.write_text('touch "$READY_FILE"\nexec sleep 30\n')
            env = {**os.environ, "RUNNER_TEMP": directory, "READY_FILE": str(marker)}
            process = subprocess.Popen(["bash", str(WRAPPER), str(path)], env=env,
                                       text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                for _ in range(100):
                    if marker.exists():
                        break
                    time.sleep(0.02)
                self.assertTrue(marker.exists())
                process.terminate()
                stdout, stderr = process.communicate(timeout=5)
                self.assertEqual(process.returncode, 143)
                self.assertEqual((stdout, stderr), ("", ""))
                self.assertFalse(list(Path(directory).glob("private-run.*")))
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate()


if __name__ == "__main__":
    unittest.main()
