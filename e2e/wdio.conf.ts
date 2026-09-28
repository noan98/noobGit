/*
 * tauri-driver + WebdriverIO の設定 (#177)。
 *
 * Tauri 2 公式ドキュメント（WebDriver / tauri-driver + WebdriverIO の
 * 「手作業で配線する」パターン）に従う:
 *   - onPrepare でデバッグビルド（`tauri build --debug --no-bundle`）を用意する。
 *   - beforeSession で tauri-driver を起動し、WebDriver リクエストを
 *     ネイティブの WebDriver 実装へプロキシしてもらう。
 *   - afterSession（およびプロセス終了時）に tauri-driver を止める。
 *
 * tauri-driver は Linux / Windows のみ対応（macOS 非対応）:
 *   - Linux: `WebKitWebDriver`（apt パッケージ `webkit2gtk-driver`）が必要。
 *     ヘッドレス実行には `xvfb-run` を使う（`npm run test:e2e` 自体は
 *     ディスプレイの有無を意識しない）。
 *   - Windows: ローカルで実行する場合は Edge のバージョンに合った
 *     `msedgedriver` が別途必要（`msedgedriver-tool` などで取得する）。
 *     CI（本リポジトリの e2e.yml）は Linux 専用。
 */
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Options } from "@wdio/types";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");

// Cargo のビルド出力先。共有環境で他のエージェント/セッションと衝突しない
// よう CARGO_TARGET_DIR が設定されていればそれを優先し、無ければ既定の
// `<repo>/target` を使う（Cargo ワークスペースはリポジトリ直下にある）。
const cargoTargetDir = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : join(repoRoot, "target");

const binaryName = process.platform === "win32" ? "noobgit.exe" : "noobgit";
const applicationPath = join(cargoTargetDir, "debug", binaryName);

const screenshotsDir = join(__dirname, "screenshots");

let tauriDriver: ChildProcess | undefined;
let stopping = false;

function stopTauriDriver(): void {
  stopping = true;
  tauriDriver?.kill();
}

// テストプロセスが異常終了した場合でも tauri-driver を残さない。
for (const signal of ["exit", "SIGINT", "SIGTERM", "SIGHUP"] as const) {
  process.on(signal, stopTauriDriver);
}

export const config: Options.Testrunner = {
  hostname: "127.0.0.1",
  port: 4444,
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      // tauri-driver 固有のオプション。ビルド済みデバッグバイナリを渡す。
      "tauri:options": {
        application: applicationPath,
      },
      // `tauri:options` は @wdio/types の Capabilities 型に含まれないため、
      // ここだけ型を緩める（tsx は型チェックをしないため実行には影響しない）。
    } as unknown as WebdriverIO.Capabilities,
  ],
  logLevel: "info",
  bail: 0,
  waitforTimeout: 15000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 120000,
  },

  // デバッグビルドを用意する。`tauri build` の `beforeBuildCommand` が
  // `npm run build` を自動で実行するため、フロントエンドの dist も揃う。
  onPrepare: () => {
    const result = spawnSync(
      "npm",
      ["run", "tauri", "--", "build", "--debug", "--no-bundle"],
      { cwd: repoRoot, stdio: "inherit", shell: true },
    );
    if (result.status !== 0) {
      throw new Error(
        "Tauri のデバッグビルドに失敗しました（npm run tauri -- build --debug --no-bundle）",
      );
    }
    if (!existsSync(applicationPath)) {
      throw new Error(
        `ビルド済みバイナリが見つかりません: ${applicationPath}`,
      );
    }
  },

  // tauri-driver を起動し、以後の WebDriver リクエストをプロキシしてもらう。
  beforeSession: () => {
    tauriDriver = spawn(join(homedir(), ".cargo", "bin", "tauri-driver"), [], {
      stdio: [null, process.stdout, process.stderr],
    });

    tauriDriver.on("error", (error) => {
      console.error("tauri-driver の起動に失敗しました:", error);
      process.exit(1);
    });
    tauriDriver.on("exit", (code) => {
      if (!stopping) {
        console.error("tauri-driver が予期せず終了しました。終了コード:", code);
        process.exit(1);
      }
    });
  },

  // このセッションのために起動した tauri-driver を止める。
  // セッション開始自体に失敗した場合 afterSession が呼ばれないことがあるため、
  // 上のプロセス終了時フックでも二重に後片付けする。
  afterSession: () => {
    stopTauriDriver();
  },

  // 失敗したテストのスクリーンショットを残す（CI 側で actions/upload-artifact
  // により保存する）。保存自体の失敗はテスト結果に影響させない。
  afterTest: async (test, _context, result) => {
    if (result.passed) return;
    try {
      mkdirSync(screenshotsDir, { recursive: true });
      // Mocha の Test は `fullTitle()` がメソッドなので、代わりに
      // `parent`（describe 名）+ `title`（it 名）から組み立てる。
      const rawName = [test.parent, test.title].filter(Boolean).join("-");
      const safeName = rawName.replace(/[^\w.-]+/g, "_");
      await browser.saveScreenshot(
        join(screenshotsDir, `${safeName}-${Date.now()}.png`),
      );
    } catch (e) {
      console.error("スクリーンショットの保存に失敗しました:", e);
    }
  },
};
