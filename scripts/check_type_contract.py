#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Rust (core/) の enum バリアントと TypeScript (src/api.ts) の型リテラルが
一致しているかを検証する。

安全性に直結する enum の型契約を CI で自動検出するための軽量スクリプト。
check_handlers.py と同じ方針: Python 標準ライブラリのみ、正規表現でソース解析、
不一致を日本語で報告し exit code 1、完全一致なら 0。

検査対象:
  - OperationKind  (core/src/safety.rs)   <-> src/api.ts
  - RiskLevel      (core/src/safety.rs)   <-> src/api.ts
  - ChangeKind     (core/src/model.rs)    <-> src/api.ts
  - DiffLineKind   (core/src/model.rs)    <-> src/api.ts
  - NetworkErrorKind (core/src/error.rs)  <-> src/api.ts
"""
import re
import sys
from pathlib import Path


ROOT = Path(__file__).parent.parent

RUST_FILES = {
    "OperationKind":    ROOT / "core" / "src" / "safety.rs",
    "RiskLevel":        ROOT / "core" / "src" / "safety.rs",
    "ChangeKind":       ROOT / "core" / "src" / "model.rs",
    "DiffLineKind":     ROOT / "core" / "src" / "model.rs",
    "NetworkErrorKind": ROOT / "core" / "src" / "error.rs",
}

TS_FILE = ROOT / "src" / "api.ts"

# ファイルの存在確認。
for enum_name, path in RUST_FILES.items():
    if not path.exists():
        print(f"エラー: {path} が見つかりません ({enum_name})")
        sys.exit(1)
if not TS_FILE.exists():
    print(f"エラー: {TS_FILE} が見つかりません")
    sys.exit(1)


def to_snake_case(name: str) -> str:
    """PascalCase / CamelCase を snake_case に変換する。
    例: AmendCommit -> amend_commit, SshKeyNotFound -> ssh_key_not_found
    """
    # 連続大文字（頭字語）の後ろに小文字が続く境界も分割する。
    # 例: "SSHKey" -> "SSH_Key" -> "ssh_key"
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', name)
    s = re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', s)
    return s.lower()


def extract_rust_variants(source: str, enum_name: str) -> set[str]:
    """Rust ソースから指定 enum のバリアント名を抽出し、snake_case に変換して返す。

    `#[serde(rename_all = "snake_case")]` が付いていることを前提にする。
    バリアント行は「先頭が大文字で始まり、識別子文字が続き、行末かカンマ・改行まで」
    という形を正規表現で拾う。コメント行・属性行（#[ ... ]）は除外する。
    """
    # enum XXX { ... } のブロックを抽出する。
    pattern = rf'\benum\s+{re.escape(enum_name)}\s*\{{([^}}]*)\}}'
    m = re.search(pattern, source, re.DOTALL)
    if not m:
        print(f"エラー: enum {enum_name} が Rust ソースで見つかりません")
        sys.exit(1)

    block = m.group(1)
    variants: set[str] = set()
    for line in block.splitlines():
        stripped = line.strip()
        # 空行・コメント行・属性行はスキップ。
        if not stripped:
            continue
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("#"):
            continue
        # バリアント名は先頭が大文字から始まる識別子（フィールドや { } を持たない）。
        # "Variant," / "Variant" / "Variant // comment" にマッチする。
        variant_m = re.match(r'^([A-Z][A-Za-z0-9_]*)', stripped)
        if variant_m:
            raw_name = variant_m.group(1)
            variants.add(to_snake_case(raw_name))

    return variants


def extract_ts_literals(source: str, type_name: str) -> set[str]:
    """TypeScript ソースから指定型エイリアスの文字列リテラルを抽出して返す。

    `export type Foo = "bar" | "baz" | ...;` の形を想定。
    複数行にまたがっていても正しく処理する。
    """
    # export type NAME = ... ; のブロックを抽出する。
    pattern = rf'export\s+type\s+{re.escape(type_name)}\s*=([^;]+);'
    m = re.search(pattern, source, re.DOTALL)
    if not m:
        print(f"エラー: TypeScript 型 {type_name} が {TS_FILE} で見つかりません")
        sys.exit(1)

    block = m.group(1)
    # "literal" / 'literal' の文字列リテラルを全て拾う。
    literals = set(re.findall(r'["\']([a-z_][a-z0-9_]*)["\']', block))
    return literals


def check_enum(enum_name: str) -> bool:
    """指定 enum の Rust ↔ TS の一致を確認する。

    一致していれば True を返す。差分があれば差分を表示して False を返す。
    """
    rust_path = RUST_FILES[enum_name]
    rust_source = rust_path.read_text(encoding="utf-8")
    ts_source = TS_FILE.read_text(encoding="utf-8")

    rust_variants = extract_rust_variants(rust_source, enum_name)
    ts_literals = extract_ts_literals(ts_source, enum_name)

    only_in_rust = rust_variants - ts_literals
    only_in_ts = ts_literals - rust_variants

    if not only_in_rust and not only_in_ts:
        print(f"✅ {enum_name}: {len(rust_variants)} バリアントが一致しています")
        return True

    print(f"❌ {enum_name}: Rust と TypeScript の型が一致しません")
    if only_in_rust:
        print(f"   Rust にあって TypeScript に無いバリアント ({len(only_in_rust)} 件):")
        for v in sorted(only_in_rust):
            print(f"     - {v}")
    if only_in_ts:
        print(f"   TypeScript にあって Rust に無いバリアント ({len(only_in_ts)} 件):")
        for v in sorted(only_in_ts):
            print(f"     - {v}")
    return False


def main() -> None:
    target_enums = [
        "OperationKind",
        "RiskLevel",
        "ChangeKind",
        "DiffLineKind",
        "NetworkErrorKind",
    ]

    all_ok = True
    for enum_name in target_enums:
        if not check_enum(enum_name):
            all_ok = False

    if all_ok:
        print("\nすべての enum 型契約が一致しています。")
    else:
        print(
            "\n型契約に乖離があります。"
            " core/ の Rust 型を変更したときは src/api.ts も必ず更新してください。"
        )

    sys.exit(0 if all_ok else 1)


if __name__ == "__main__":
    main()
