"""打包脚本回归测试：make_app.ps1 的行为契约。

对应系分 §11.1 中的 Windows 打包测试层，静态校验脚本关键不变量：
- 参数校验（ValidateSet）与 release/debug 分支；
- 构建前全量测试（可 -SkipTests 跳过）；
- 构建失败保留 dist 旧产物（脚本永不提前删除 dist；
  复制产物只发生在 tauri build 成功且产物存在之后）。

在任何平台都可通过 `python -m unittest tests.test_packaging_windows` 运行。
"""

import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig")


class MakeAppScriptContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.script_path = REPO / "make_app.ps1"
        cls.src = read(cls.script_path)

    def test_param_validation_and_default_mode(self):
        self.assertRegex(
            self.src,
            r"\[ValidateSet\('Release',\s*'Debug'\)\]\s*\r?\n?\s*\[string\]\$Mode = 'Release'",
            "Mode 参数必须 ValidateSet 且默认 Release",
        )
        self.assertIn("[switch]$SkipTests", self.src)

    def test_strict_error_handling(self):
        self.assertIn("$ErrorActionPreference = 'Stop'", self.src)

    def test_full_tests_gate_the_build(self):
        self.assertIn("cargo test --manifest-path src-tauri/Cargo.toml", self.src)
        self.assertIn("node --test tests/frontend.test.cjs", self.src)
        self.assertRegex(
            self.src,
            r"python -m unittest discover -s tests -p 'test_\*_windows\.py'",
        )
        # 每一项失败都不得进入构建
        self.assertGreaterEqual(self.src.count("Assert-LastExitCode"), 4)
        self.assertRegex(self.src, r"if \(-not \$SkipTests\)")

    def test_release_and_debug_build_branches(self):
        self.assertRegex(
            self.src,
            r"\$tauriArgs = if \(\$Mode -eq 'Debug'\) \{ @\('build', '--debug'\) \} else \{ @\('build'\) \}",
            "Debug 分支必须携带 --debug",
        )
        self.assertIn("cargo tauri @tauriArgs", self.src, "优先使用 cargo 子命令")
        self.assertIn(
            "npx --yes @tauri-apps/cli@2 @tauriArgs",
            self.src,
            "本机没有 cargo-tauri 时必须回退到 npm 分发的 CLI",
        )

    def test_dist_never_wiped(self):
        self.assertNotIn("Remove-Item", self.script_path.read_text(encoding="utf-8-sig"))
        self.assertNotIn("Remove-Item", self.src)

    def test_artifacts_copied_only_after_successful_build(self):
        build_pos = self.src.index("cargo tauri @tauriArgs")
        copy_pos = self.src.index("Copy-Item")
        self.assertLess(build_pos, copy_pos, "产物复制必须发生在构建之后")
        # 产物确认失败同样不得触碰 dist
        self.assertIn("没有找到 NSIS 产物", self.src)

    def test_failure_paths_keep_dist_intact(self):
        self.assertIn("dist 未做任何改动", self.src)
        for marker in (
            "cargo test --manifest-path src-tauri/Cargo.toml",
            "node --test tests/frontend.test.cjs",
            "python -m unittest discover -s tests",
            "tauri build（$Mode）",
        ):
            idx = self.src.index(marker)
            window = self.src[max(0, idx - 200) : idx + 200]
            self.assertIn(
                "Assert-LastExitCode", window, f"{marker} 必须紧跟失败断言"
            )

    def test_installer_lookup_uses_nsis_bundle_dir(self):
        self.assertRegex(self.src, r"Join-Path \$profile 'bundle\\nsis'")
        self.assertRegex(self.src, r"Get-ChildItem -Path \$bundleDir -Filter '\*\.exe'")

    def test_profile_mapping_for_cargo_targets(self):
        self.assertRegex(
            self.src,
            r"\$profile = if \(\$Mode -eq 'Release'\) \{ 'release' \} else \{ 'debug' \}",
        )


if __name__ == "__main__":
    unittest.main()
