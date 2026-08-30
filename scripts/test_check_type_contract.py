#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
scripts/check_type_contract.py 自体のユニットテスト。

check_type_contract.py は「Rust enum の snake_case バリアントと TypeScript の
文字列リテラル union が一致しているか」を検証する CI 上の要である。この
テストが無いと、スクリプト本体にバグが混入して不一致を見逃す状態になっても
誰も気づけない。

方針:
  - 標準ライブラリのみ（unittest / tempfile / subprocess）を使う。
  - 擬似的な core/src/{safety,model,error}.rs と src/api.ts を一時ディレクトリに
    作り、check_type_contract.py にルートパスを第一引数として渡して
    サブプロセスで実行する（本体を書き換えずに済む、最小限の拡張のみ利用）。
  - 意図的に不一致な入力を与えたとき正しく非ゼロ終了することを検証する。
"""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).parent / "check_type_contract.py"

# check_type_contract.py が要求する5つの enum を、デフォルトで矛盾なく
# 一致させたベースの Rust / TypeScript ソース断片。
BASE_SAFETY_RS = """
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    StageAll,
    Commit,
    Rebase,
}

#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Caution,
    Destructive,
}
"""

BASE_MODEL_RS = """
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    TypeChange,
}

#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}
"""

BASE_ERROR_RS = """
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind {
    SshKeyNotFound,
    AuthFailed,
}
"""

BASE_API_TS = """
export type OperationKind = "stage_all" | "commit" | "rebase";
export type RiskLevel = "safe" | "caution" | "destructive";
export type ChangeKind = "added" | "modified" | "type_change";
export type DiffLineKind = "context" | "addition" | "deletion";
export type NetworkErrorKind = "ssh_key_not_found" | "auth_failed";
"""


def write_fixture(root: Path, *, safety_rs=None, model_rs=None, error_rs=None, api_ts=None) -> None:
    """一時ディレクトリに擬似的な core/ と src/api.ts を書き出す。

    各引数を渡すとベース内容の代わりにその内容を使う（不一致ケースの注入用）。
    """
    core_src = root / "core" / "src"
    core_src.mkdir(parents=True, exist_ok=True)
    (root / "src").mkdir(parents=True, exist_ok=True)

    (core_src / "safety.rs").write_text(safety_rs if safety_rs is not None else BASE_SAFETY_RS, encoding="utf-8")
    (core_src / "model.rs").write_text(model_rs if model_rs is not None else BASE_MODEL_RS, encoding="utf-8")
    (core_src / "error.rs").write_text(error_rs if error_rs is not None else BASE_ERROR_RS, encoding="utf-8")
    (root / "src" / "api.ts").write_text(api_ts if api_ts is not None else BASE_API_TS, encoding="utf-8")


def run_check(root: Path) -> subprocess.CompletedProcess:
    """check_type_contract.py をサブプロセスで実行し、結果を返す。"""
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(root)],
        capture_output=True,
        text=True,
        timeout=30,
    )


class TestCheckTypeContract(unittest.TestCase):
    def test_matching_case_exits_zero(self):
        """Rust と TypeScript が完全に一致していれば exit code 0。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_fixture(root)
            result = run_check(root)
            self.assertEqual(
                result.returncode, 0,
                msg=f"一致しているはずなのに失敗した:\nstdout={result.stdout}\nstderr={result.stderr}",
            )
            self.assertIn("すべての enum 型契約が一致しています", result.stdout)

    def test_rust_only_variant_fails(self):
        """Rust 側だけに存在するバリアントがあれば非ゼロ終了する。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            safety_rs_with_extra = BASE_SAFETY_RS.replace(
                "    StageAll,\n", "    StageAll,\n    ForcePush,\n"
            )
            write_fixture(root, safety_rs=safety_rs_with_extra)
            result = run_check(root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("force_push", result.stdout)

    def test_ts_only_variant_fails(self):
        """TypeScript 側だけに存在するリテラルがあれば非ゼロ終了する。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            api_ts_with_extra = BASE_API_TS.replace(
                'export type RiskLevel = "safe" | "caution" | "destructive";',
                'export type RiskLevel = "safe" | "caution" | "destructive" | "critical";',
            )
            write_fixture(root, api_ts=api_ts_with_extra)
            result = run_check(root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("critical", result.stdout)

    def test_variant_name_typo_fails(self):
        """バリアント名の表記ゆれ（大文字小文字の typo）を見逃さない。

        TypeScript 側に本来 "caution" であるべきリテラルを誤って "Caution"
        （先頭大文字）と書いてしまったケース。スクリプトの抽出正規表現は
        小文字始まりのリテラルのみ拾うため、これは「caution が欠落している」
        不一致として検出されなければならない。
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            api_ts_with_typo = BASE_API_TS.replace(
                'export type RiskLevel = "safe" | "caution" | "destructive";',
                'export type RiskLevel = "safe" | "Caution" | "destructive";',
            )
            write_fixture(root, api_ts=api_ts_with_typo)
            result = run_check(root)
            self.assertNotEqual(
                result.returncode, 0,
                msg="typo によるバリアント不一致が検出されなかった（見逃しバグの疑い）",
            )
            self.assertIn("RiskLevel", result.stdout)

    def test_missing_rust_file_reports_error(self):
        """対象の Rust ファイルが存在しなければエラー終了する（誤って0で通らない）。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_fixture(root)
            (root / "core" / "src" / "safety.rs").unlink()
            result = run_check(root)
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
