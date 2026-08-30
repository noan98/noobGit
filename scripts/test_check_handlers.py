#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
scripts/check_handlers.py のユニットテスト。

check_handlers.py は `#[tauri::command]` 関数と `generate_handler![...]`
登録の一致を検証する CI の守門であり、このスクリプト自体にバグがあると
「未登録コマンドを見逃す」まま CI がグリーンになってしまう。そのため、
一時ディレクトリに擬似的な src-tauri/src/lib.rs を用意し、check_handlers.py
をサブプロセスとして実行して終了コード・出力を検証する。

check_handlers.py は `ROOT = Path(__file__).parent.parent` で
リポジトリルートを自分自身の場所から求めているため、cwd を変えるだけでは
参照先を差し替えられない。そこで各テストでは check_handlers.py を
一時ディレクトリの scripts/ 配下にコピーし、その隣に
src-tauri/src/lib.rs を書き出すことで「一時ディレクトリ = 疑似リポジトリ
ルート」というサンドボックスを作る。これにより check_handlers.py 本体には
一切手を加えない。

標準ライブラリのみを使用する（pytest 等の追加依存は入れない）。
"""
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
CHECK_HANDLERS = REPO_ROOT / "scripts" / "check_handlers.py"


class CheckHandlersTestCase(unittest.TestCase):
    """check_handlers.py を疑似リポジトリ上で実行して検証する基底クラス。"""

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="check_handlers_test_")
        self.addCleanup(shutil.rmtree, self._tmpdir, ignore_errors=True)
        self.sandbox = Path(self._tmpdir)

        scripts_dir = self.sandbox / "scripts"
        scripts_dir.mkdir(parents=True)
        shutil.copy(CHECK_HANDLERS, scripts_dir / "check_handlers.py")

        self.lib_rs_path = self.sandbox / "src-tauri" / "src" / "lib.rs"
        self.lib_rs_path.parent.mkdir(parents=True)

    def run_check(self, lib_rs_source: str) -> subprocess.CompletedProcess:
        """疑似 lib.rs を書き出して check_handlers.py を実行する。"""
        self.lib_rs_path.write_text(lib_rs_source, encoding="utf-8")
        return subprocess.run(
            [sys.executable, "scripts/check_handlers.py"],
            cwd=self.sandbox,
            capture_output=True,
            text=True,
        )


def make_lib_rs(commands: list[str], handler_entries: list[str]) -> str:
    """#[tauri::command] 関数群と generate_handler! 登録を持つ lib.rs を組み立てる。"""
    fns = "\n\n".join(
        f'#[tauri::command]\nfn {name}(repo_path: String) -> Result<(), String> {{\n    Ok(())\n}}'
        for name in commands
    )
    handlers = ",\n            ".join(handler_entries)
    return f"""// 自動生成された疑似 lib.rs（テスト用）

{fns}

pub fn run() {{
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            {handlers}
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}}
"""


class TestExactMatch(CheckHandlersTestCase):
    """コマンド関数と登録が完全に一致するケース → ゼロ終了。"""

    def test_all_registered_exits_zero(self):
        lib_rs = make_lib_rs(
            commands=["get_status", "stage_all", "commit_changes"],
            handler_entries=["get_status", "stage_all", "commit_changes"],
        )
        result = self.run_check(lib_rs)
        self.assertEqual(
            result.returncode, 0,
            msg=f"stdout={result.stdout!r} stderr={result.stderr!r}",
        )
        self.assertIn("✅", result.stdout)


class TestUnregisteredCommand(CheckHandlersTestCase):
    """#[tauri::command] があるが generate_handler! に未登録 → 非ゼロ終了。"""

    def test_unregistered_command_exits_nonzero(self):
        lib_rs = make_lib_rs(
            commands=["get_status", "stage_all"],
            handler_entries=["get_status"],
        )
        result = self.run_check(lib_rs)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stage_all", result.stdout)
        self.assertIn("未登録", result.stdout)


class TestOrphanedHandlerEntry(CheckHandlersTestCase):
    """generate_handler! に登録があるが実装関数が存在しない → 非ゼロ終了。"""

    def test_orphaned_handler_exits_nonzero(self):
        lib_rs = make_lib_rs(
            commands=["get_status"],
            handler_entries=["get_status", "delete_everything"],
        )
        result = self.run_check(lib_rs)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("delete_everything", result.stdout)
        self.assertIn("実装が見つからない", result.stdout)


class TestPartiallyRegisteredMultipleCommands(CheckHandlersTestCase):
    """複数コマンドのうち一部だけ登録されているケース → 漏れを正しく検出する。"""

    def test_reports_only_the_missing_commands(self):
        lib_rs = make_lib_rs(
            commands=[
                "get_status", "stage_all", "unstage_path",
                "commit_changes", "push_branch",
            ],
            handler_entries=["get_status", "stage_all", "push_branch"],
        )
        result = self.run_check(lib_rs)
        self.assertNotEqual(result.returncode, 0)
        # 未登録の 2 件だけが報告され、登録済みの 3 件は報告されないこと。
        self.assertIn("unstage_path", result.stdout)
        self.assertIn("commit_changes", result.stdout)
        self.assertNotIn("get_status\n", result.stdout)
        self.assertNotIn("push_branch\n", result.stdout)


class TestMissingLibRs(CheckHandlersTestCase):
    """src-tauri/src/lib.rs 自体が存在しない場合 → 非ゼロ終了。"""

    def test_missing_lib_rs_exits_nonzero(self):
        # setUp() はディレクトリのみ用意し lib.rs 自体は書き出さないので、
        # そのまま実行すればファイル不在のケースを再現できる。
        result = subprocess.run(
            [sys.executable, "scripts/check_handlers.py"],
            cwd=self.sandbox,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
