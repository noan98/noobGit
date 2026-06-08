#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
src-tauri/src/lib.rs の #[tauri::command] 関数と
generate_handler![...] への登録が一致しているかを検証する。

「実装済みだが未登録」「登録済みだが未実装」の不整合を検出し、
どちらの場合も終了コード 1 を返す。
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
LIB_PATH = ROOT / "src-tauri" / "src" / "lib.rs"

if not LIB_PATH.exists():
    print(f"エラー: {LIB_PATH} が見つかりません")
    sys.exit(1)

source = LIB_PATH.read_text(encoding="utf-8")

# #[tauri::command] の直後に来る fn NAME( のパターンを抽出。
# 属性とfn宣言の間には改行・空白のみ（コメントは入らない）。
command_fns = set(re.findall(
    r'#\[tauri::command\]\s+(?:async\s+)?fn\s+(\w+)\s*\(',
    source,
))

# generate_handler![...] の中身を抽出する。
handler_match = re.search(
    r'tauri::generate_handler!\s*\[([\s\S]*?)\]',
    source,
)
if not handler_match:
    print("エラー: tauri::generate_handler![...] が見つかりません")
    sys.exit(1)

handler_text = handler_match.group(1)
# カンマ区切りの識別子リストから名前を取り出す。コメント行は無視する。
handler_names: set[str] = set()
for line in handler_text.splitlines():
    stripped = line.strip().rstrip(",")
    if stripped and not stripped.startswith("//"):
        handler_names.add(stripped)

unregistered = command_fns - handler_names
orphaned = handler_names - command_fns

ok = True

if unregistered:
    print("❌ generate_handler! に未登録のコマンド関数（実装あり・登録なし）:")
    for name in sorted(unregistered):
        print(f"  - {name}")
    ok = False

if orphaned:
    print("❌ generate_handler! に登録済みだが実装が見つからないコマンド（登録あり・実装なし）:")
    for name in sorted(orphaned):
        print(f"  - {name}")
    ok = False

if ok:
    print(f"✅ {len(command_fns)} 個のコマンドがすべて正しく登録されています")

sys.exit(0 if ok else 1)
