// #270: リポジトリタブのキーボード巡回・ドラッグ並べ替えのロジック。
//
// App.tsx が持つタブ配列に対して行う「巡回」「並べ替え」を純粋関数として
// 切り出す。React の状態や DOM に一切触れないため、Vitest で単体テストできる。

/**
 * キーイベントが Ctrl+Tab / Ctrl+Shift+Tab（アクティブタブの巡回）かどうかを
 * 判定する。Mac の Cmd（metaKey）も Ctrl と同等に扱う（useGlobalShortcuts の
 * isPaletteShortcut と同じ考え方）。
 *
 * 戻り値はそのまま cycleActiveTabId の direction に渡せる: 1 なら次のタブ
 * （Ctrl+Tab）、-1 なら前のタブ（Ctrl+Shift+Tab）。対象のショートカットで
 * なければ null を返す。
 */
export function tabCycleDirection(e: KeyboardEvent): 1 | -1 | null {
  const ctrl = e.ctrlKey || e.metaKey;
  if (!ctrl || e.key !== "Tab") return null;
  return e.shiftKey ? -1 : 1;
}

/**
 * Ctrl+Tab / Ctrl+Shift+Tab でアクティブタブを巡回させたときの、次のアクティブ
 * タブ id を返す。
 *
 * - direction: 1 なら次のタブへ（Ctrl+Tab）、-1 なら前のタブへ（Ctrl+Shift+Tab）。
 * - 末尾から先頭へ（逆方向は先頭から末尾へ）ラップする。
 * - タブが 1 つ以下、または activeId が一覧に無い場合は activeId をそのまま
 *   返す（呼び出し側が「何もしない」判定にそのまま使える）。
 */
export function cycleActiveTabId(
  ids: readonly string[],
  activeId: string,
  direction: 1 | -1,
): string {
  if (ids.length <= 1) return activeId;
  const index = ids.indexOf(activeId);
  if (index === -1) return activeId;
  const next = (index + direction + ids.length) % ids.length;
  return ids[next];
}

/**
 * ドラッグ並べ替え後の id 順（orderedIds）に合わせて items を並べ替える。
 *
 * framer-motion の Reorder.Group は並べ替え後の順序を onReorder で返すが、
 * タブの実体（App 側が持つ状態）はここでは扱わず id の配列だけを受け取り、
 * items 側の実体を並べ替えて返す。
 *
 * 防御的に扱う: orderedIds に無い id を持つ items は元の相対順を保ったまま
 * 末尾に残し、items に存在しない id は無視する（取りこぼしがあってもタブが
 * 消えたり増えたりしないようにする）。
 */
export function reorderByIds<T extends { id: string }>(
  items: readonly T[],
  orderedIds: readonly string[],
): T[] {
  const byId = new Map(items.map((item) => [item.id, item] as const));
  const seen = new Set<string>();
  const ordered: T[] = [];

  for (const id of orderedIds) {
    const item = byId.get(id);
    if (item && !seen.has(id)) {
      ordered.push(item);
      seen.add(id);
    }
  }
  for (const item of items) {
    if (!seen.has(item.id)) {
      ordered.push(item);
      seen.add(item.id);
    }
  }
  return ordered;
}
