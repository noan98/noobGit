/**
 * ロード中のプレースホルダー行。Chakra UI v3 の `Skeleton` を使い、幅を
 * 50〜90% の範囲でランダムに変動させて実データっぽい見た目を作る。
 *
 * 幅はマウント時に一度だけ `useMemo` で確定させる。毎レンダーで乱数を
 * 引き直すと Skeleton がちらついてしまうため、依存配列は空にして固定する。
 */
import { useMemo } from "react";
import { Skeleton } from "@chakra-ui/react";

interface SkeletonRowProps {
  /** 行の高さ（Skeleton の height にそのまま渡す）。 */
  height?: string;
  /** 幅の変動範囲（%）の下限。 */
  minWidthPct?: number;
  /** 幅の変動範囲（%）の上限。 */
  maxWidthPct?: number;
  borderRadius?: string;
}

export function SkeletonRow({
  height = "1em",
  minWidthPct = 50,
  maxWidthPct = 90,
  borderRadius = "4px",
}: SkeletonRowProps) {
  // レンダーごとに再計算されないよう、マウント時に一度だけ幅を決める
  // （意図的に依存配列は空にし、以後の再レンダーでは再計算しない）。
  const widthPct = useMemo(
    () => minWidthPct + Math.random() * (maxWidthPct - minWidthPct),
    [],
  );

  return (
    <Skeleton
      height={height}
      width={`${widthPct.toFixed(0)}%`}
      borderRadius={borderRadius}
    />
  );
}

interface SkeletonStackProps {
  /** スタックする行数（3〜5 段を想定）。 */
  rows?: number;
  height?: string;
  minWidthPct?: number;
  maxWidthPct?: number;
}

/** SkeletonRow を縦に並べたスタック。汎用のリストプレースホルダーとして使う。 */
export function SkeletonStack({
  rows = 4,
  height = "1em",
  minWidthPct = 50,
  maxWidthPct = 90,
}: SkeletonStackProps) {
  return (
    <div className="skeleton-stack">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="skeleton-stack-row">
          <SkeletonRow
            height={height}
            minWidthPct={minWidthPct}
            maxWidthPct={maxWidthPct}
          />
        </div>
      ))}
    </div>
  );
}
