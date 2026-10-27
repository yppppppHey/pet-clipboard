import os
import plistlib
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="pet-packaging-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "project with spaces"
        self.root.mkdir()
        script = Path(__file__).resolve().parents[1] / "make_app.sh"
        shutil.copy2(script, self.root / "make_app.sh")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.app = self.root / "Desktop Pet Clipboard.app"
        self.app.mkdir()
        (self.app / "previous-marker").write_text("old app")
        self.trace = self.root / "trace"
        self.env = dict(os.environ)
        self.env.update({
            "PATH": str(self.tools) + ":/usr/bin:/bin",
            "CARGO_TARGET_DIR": str(self.root / "target"),
            "TRACE": str(self.trace),
            "FAIL_BUILD": "0", "FAIL_SIGN": "0", "FAIL_VERIFY": "0",
            "FAIL_REPLACE": "0", "MOVE_MARKER": str(self.root / "move-failed"),
        })
        for profile in ("debug", "release"):
            binary = self.root / "target" / profile / "desktop-pet-clipboard"
            binary.parent.mkdir(parents=True)
            binary.write_text(profile + " binary")
        self.tool("uname", 'echo Darwin')
        self.tool("cargo", '''
printf 'cargo %s\\n' "$*" >> "$TRACE"
[ "$FAIL_BUILD" != 1 ]
''')
        self.tool("plutil", 'exit 0')
        self.tool("codesign", '''
printf 'codesign %s\\n' "$*" >> "$TRACE"
case "$1" in
  --force) [ "$FAIL_SIGN" != 1 ] ;;
  --verify) [ "$FAIL_VERIFY" != 1 ] ;;
esac
''')
        self.tool("mv", '''
case "$1" in
  */.desktop-pet-build.*/Desktop\\ Pet\\ Clipboard.app)
    if [ "$FAIL_REPLACE" = 1 ] && [ ! -e "$MOVE_MARKER" ]; then
      touch "$MOVE_MARKER"
      exit 1
    fi ;;
esac
exec /bin/mv "$@"
''')

    def tool(self, name, body):
        tool = self.tools / name
        tool.write_text("#!/bin/bash\nset -eu\n" + body + "\n")
        tool.chmod(0o755)

    def run_script(self, *args):
        return subprocess.run(
            ["/bin/bash", str(self.root / "make_app.sh"), *args],
            cwd=self.root, env=self.env, capture_output=True, text=True,
        )

    def assert_original(self):
        self.assertEqual((self.app / "previous-marker").read_text(), "old app")
        self.assertEqual(list(self.root.glob(".desktop-pet-build.*")), [])

    def test_build_failure_preserves_existing_app(self):
        self.env["FAIL_BUILD"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assert_original()

    def test_signing_failure_preserves_existing_app(self):
        self.env["FAIL_SIGN"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assert_original()

    def test_verification_failure_preserves_existing_app(self):
        self.env["FAIL_VERIFY"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assert_original()

    def test_failed_replacement_restores_existing_app(self):
        self.env["FAIL_REPLACE"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assertTrue((self.root / "move-failed").exists())
        self.assert_original()

    def test_release_build_is_default_and_signed_before_installation(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.app / "Contents/MacOS/desktop-pet-clipboard"
        self.assertEqual(binary.read_text(), "release binary")
        with (self.app / "Contents/Info.plist").open("rb") as file:
            self.assertIs(plistlib.load(file)["LSUIElement"], True)
        trace = self.trace.read_text()
        self.assertIn("--release", trace)
        self.assertIn("--locked", trace)
        self.assertIn("codesign --verify --deep --strict", trace)
        self.assertFalse((self.app / "previous-marker").exists())
        self.assertEqual(list(self.root.glob(".desktop-pet-build.*")), [])

    def test_debug_build_does_not_pass_release_flag(self):
        result = self.run_script("--debug")
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.app / "Contents/MacOS/desktop-pet-clipboard"
        self.assertEqual(binary.read_text(), "debug binary")
        self.assertNotIn("--release", self.trace.read_text())

    def test_unknown_arguments_do_not_trigger_a_build(self):
        self.assertEqual(self.run_script("--invalid").returncode, 2)
        self.assertEqual(self.run_script("--debug", "extra").returncode, 2)
        self.assertFalse(self.trace.exists())
        self.assert_original()


if __name__ == "__main__":
    unittest.main()
