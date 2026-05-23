# noobGit

**ジュニアエンジニアが安心して使えるGitツール**

Gitの「うっかり事故」を防ぎ、何が起きるのかを日本語でわかりやすく示すデスクトップGUIです。
Rust + [Tauri 2](https://v2.tauri.app/) で作られており、対象プラットフォームは当面 **Windows** です。

## 何が「安心」なのか（4つの柱）

1. **破壊的操作のガード** — `reset --hard`・ブランチ削除・force push・保護ブランチ(main/master)への直接pushなど、危険な操作の前に「何が起きるか・取り消せるか・代替案」を提示する確認ダイアログを出します。
2. **平易な日本語の説明** — すべての操作に「これは何をするか / なぜ安全(危険)か / 困ったときどうするか」の解説を添えます。
3. **取り消し(Undo)** — コミット・リセット・ブランチ作成/削除を、直後ならワンクリックで元に戻せます。
4. **リポジトリ状態の可視化** — 変更・履歴・ブランチを3つのパネルで一目で把握できます。

## アーキテクチャ

Gitのロジックと安全規則は、GUIから切り離した純粋Rustクレート `noobgit-core` に集約しています。
Tauri層は薄いコマンド変換のみ、フロントエンドは React + TypeScript + Vite です。

```text
noobGit/
├─ core/         # noobgit-core: Git操作・安全判定・説明・Undo（テスト可能な純粋Rust）
├─ src-tauri/    # Tauri 2 アプリ（coreを呼ぶだけの薄いコマンド層）
└─ src/          # React + TypeScript フロントエンド
```

`core` の各モジュール:

| モジュール | 役割 |
|---|---|
| `repo.rs` | 状態の読み取り（status / branches / log） |
| `ops.rs` | 書き込み操作（stage / commit / branch / switch / delete / reset） |
| `safety.rs` | 操作のリスク分類（Safe / Caution / Destructive） |
| `explain.rs` | 操作の日本語説明 |
| `undo.rs` | reflogに対応する直前位置を記録したUndo |

## 必要環境（Windows）

- [Rust](https://rustup.rs/)（MSVCツールチェーン）
- [Node.js](https://nodejs.org/) 18 以上
- Microsoft Visual Studio C++ Build Tools
- WebView2 ランタイム（Windows 11 標準。Windows 10 は[こちら](https://developer.microsoft.com/microsoft-edge/webview2/)）

> `git` 本体のインストールは不要です（libgit2 を内蔵してビルドします）。

## 開発・ビルド

```bash
# 依存をインストール
npm install

# 開発モードで起動（ホットリロード）
npm run tauri dev

# Windows用インストーラ(.exe/.msi)を生成
npm run tauri build
```

アイコンを変更したい場合は、任意の正方形PNGから再生成できます:

```bash
npm run tauri icon path/to/your-icon.png
```

## テスト

Gitロジックはすべて `core` 側のテストで検証しています（一時リポジトリを使った統合テスト）。

```bash
cargo test -p noobgit-core
```

フロントエンドの型チェック・ビルド:

```bash
npm run typecheck
npm run build
```

## ライセンス

MIT
