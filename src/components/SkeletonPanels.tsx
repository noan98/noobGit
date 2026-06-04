/**
 * スケルトンスクリーンコンポーネント群。
 *
 * リポジトリの初期読み込み中に StatusPanel / HistoryPanel / BranchPanel の
 * プレースホルダーとして表示する。実際のコンテンツと同じ構造のシルエットを
 * 先見せすることで、体感速度を改善する。
 *
 * Chakra UI v3 の Skeleton / SkeletonText を使用。Framer Motion の
 * AnimatePresence でコンテンツ出現時にフェードイン遷移する。
 */
import { Skeleton, SkeletonText } from "@chakra-ui/react";

// SkeletonText の noOfLines には数値を直接渡す（Chakra v3 API）。

/** StatusPanel 用スケルトン — ファイル一覧の形状を模倣する */
export function StatusPanelSkeleton() {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>変更</h2>
      </div>
      <div className="group">
        <h3>
          <Skeleton height="1em" width="10em" />
        </h3>
        <ul>
          {[80, 60, 72, 55, 90].map((w, i) => (
            <li key={i} style={{ display: "flex", alignItems: "center", gap: "8px", padding: "4px 0" }}>
              {/* バッジ幅のプレースホルダー */}
              <Skeleton height="1.2em" width="4em" borderRadius="4px" />
              {/* ファイルパス幅のプレースホルダー */}
              <Skeleton height="1em" width={`${w}%`} />
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** HistoryPanel 用スケルトン — コミット行（hash・メッセージ・メタ）の形状を模倣する */
export function HistoryPanelSkeleton() {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>履歴</h2>
      </div>
      <ul className="commits">
        {[75, 55, 90, 65, 80].map((w, i) => (
          <li key={i}>
            {/* コミットハッシュ */}
            <Skeleton height="1em" width="5em" style={{ fontFamily: "monospace" }} />
            <div className="commit-body">
              {/* コミットメッセージ */}
              <Skeleton height="1em" width={`${w}%`} />
              {/* 著者・日時 */}
              <Skeleton height="0.85em" width="12em" style={{ marginTop: "4px" }} />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** BranchPanel 用スケルトン — ブランチ行の形状を模倣する */
export function BranchPanelSkeleton() {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>ブランチ</h2>
      </div>
      {/* ブランチ作成フォームのプレースホルダー */}
      <div className="branch-create">
        <Skeleton height="2em" width="100%" borderRadius="4px" />
      </div>
      <ul className="branches">
        {[60, 45, 70].map((w, i) => (
          <li key={i}>
            <div className="branch-row">
              <SkeletonText noOfLines={1} width={`${w}%`} />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
