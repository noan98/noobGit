/**
 * スケルトンスクリーンコンポーネント群。
 *
 * リポジトリの初期読み込み中に StatusPanel / HistoryPanel / BranchPanel /
 * StashPanel / TagPanel のプレースホルダーとして表示する。実際のコンテンツと
 * 同じ構造のシルエットを先見せすることで、体感速度を改善する。
 *
 * Chakra UI v3 の Skeleton / SkeletonText と共通の `SkeletonRow`（幅を
 * 50〜90% でランダム変動させる行）を使用。Framer Motion の AnimatePresence
 * でコンテンツ出現時にフェードイン遷移する（呼び出し側の App.tsx を参照）。
 */
import { Skeleton, SkeletonText } from "@chakra-ui/react";
import { SkeletonRow } from "./SkeletonRow";

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
          {Array.from({ length: 5 }).map((_, i) => (
            <li key={i} style={{ display: "flex", alignItems: "center", gap: "8px", padding: "4px 0" }}>
              {/* バッジ幅のプレースホルダー */}
              <Skeleton height="1.2em" width="4em" borderRadius="4px" />
              {/* ファイルパス幅のプレースホルダー */}
              <SkeletonRow height="1em" />
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
        {Array.from({ length: 5 }).map((_, i) => (
          <li key={i}>
            {/* コミットハッシュ */}
            <Skeleton height="1em" width="5em" style={{ fontFamily: "monospace" }} />
            <div className="commit-body">
              {/* コミットメッセージ */}
              <SkeletonRow height="1em" />
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
        {Array.from({ length: 3 }).map((_, i) => (
          <li key={i}>
            <div className="branch-row">
              <SkeletonRow height="1em" minWidthPct={45} maxWidthPct={70} />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** StashPanel 用スケルトン — 退避（stash）行の形状を模倣する */
export function StashPanelSkeleton() {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>退避（stash）</h2>
      </div>
      <div className="stash-save">
        <Skeleton height="2em" width="100%" borderRadius="4px" />
      </div>
      <ul className="stashes">
        {Array.from({ length: 3 }).map((_, i) => (
          <li key={i} style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            {/* ハッシュ */}
            <Skeleton height="1em" width="4em" style={{ fontFamily: "monospace" }} />
            {/* メッセージ */}
            <SkeletonRow height="1em" />
          </li>
        ))}
      </ul>
    </div>
  );
}

/** TagPanel 用スケルトン — タグ行の形状を模倣する */
export function TagPanelSkeleton() {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>タグ</h2>
      </div>
      <div className="tag-create">
        <Skeleton height="2em" width="100%" borderRadius="4px" />
      </div>
      <ul className="tags">
        {Array.from({ length: 4 }).map((_, i) => (
          <li key={i} style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <SkeletonText noOfLines={1} width="60%" />
            {/* コミットハッシュ */}
            <Skeleton height="1em" width="4em" style={{ fontFamily: "monospace" }} />
          </li>
        ))}
      </ul>
    </div>
  );
}
