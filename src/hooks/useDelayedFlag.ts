/**
 * 「delayed start」を実装する再利用可能なフック。
 *
 * `loading` が true になってから `delayMs`（既定 100ms）経っても true の
 * ままなら表示フラグを立てる。`delayMs` 以内に `loading` が false に戻れば、
 * フラグは一度も true にならない。
 *
 * スケルトンなどのローディングプレースホルダーを、体感できないほど短い
 * ロード時間（例: キャッシュ済みデータの再取得）でちらつかせないために使う。
 * 「フリーズと区別がつかない」ことを避けつつ、高速ロードでは何も表示しない
 * バランスを取る。
 */
import { useEffect, useRef, useState } from "react";

export function useDelayedFlag(loading: boolean, delayMs = 100): boolean {
  const [show, setShow] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (loading) {
      // delayMs 経過後もまだロード中であれば表示する。
      timerRef.current = setTimeout(() => setShow(true), delayMs);
    } else {
      // ロードが終わったら即座にフラグを下ろす（タイマーが残っていれば止める）。
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      setShow(false);
    }
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [loading, delayMs]);

  return show;
}
