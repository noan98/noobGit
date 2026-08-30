import { describe, expect, it } from "vitest";
import {
  BODY_WRAP_LIMIT,
  countCodePoints,
  getCommitSubject,
  getSubjectLength,
  SUBJECT_LIMIT,
} from "../lib/commitMessage";

describe("countCodePoints", () => {
  it("ASCII 文字列は length と一致する", () => {
    expect(countCodePoints("hello")).toBe(5);
  });

  it("結合文字を含まない日本語はコードポイント数と一致する", () => {
    expect(countCodePoints("ログイン画面を追加")).toBe(9);
  });

  it("サロゲートペアの絵文字を 1 文字として数える", () => {
    // "🎉" は UTF-16 では 2 code unit（サロゲートペア）だが 1 コードポイント。
    expect("🎉".length).toBe(2);
    expect(countCodePoints("🎉")).toBe(1);
  });

  it("絵文字混じりの文字列でも UTF-16 の length より正確に数える", () => {
    const s = "修正🎉完了";
    expect(s.length).toBe(6); // UTF-16 の length では 6
    expect(countCodePoints(s)).toBe(5); // コードポイント単位では 5
  });

  it("空文字列は 0", () => {
    expect(countCodePoints("")).toBe(0);
  });
});

describe("getCommitSubject", () => {
  it("改行がなければ全体を件名として返す", () => {
    expect(getCommitSubject("ログイン画面を追加")).toBe("ログイン画面を追加");
  });

  it("最初の改行までを件名として返す（本文は含まない）", () => {
    expect(getCommitSubject("件名\n\n本文1行目\n本文2行目")).toBe("件名");
  });

  it("空文字列は空文字列を返す", () => {
    expect(getCommitSubject("")).toBe("");
  });
});

describe("getSubjectLength", () => {
  it("件名のみコードポイント単位で数える", () => {
    const message = "🎉".repeat(60) + "\n\n本文はどれだけ長くてもカウントしない";
    expect(getSubjectLength(message)).toBe(60);
  });

  it("50 文字ちょうどの件名を正しく数える", () => {
    const subject = "あ".repeat(SUBJECT_LIMIT);
    expect(getSubjectLength(subject)).toBe(SUBJECT_LIMIT);
  });

  it("51 文字なら SUBJECT_LIMIT を超える", () => {
    const subject = "あ".repeat(SUBJECT_LIMIT + 1);
    expect(getSubjectLength(subject)).toBeGreaterThan(SUBJECT_LIMIT);
  });
});

describe("定数", () => {
  it("SUBJECT_LIMIT は 50, BODY_WRAP_LIMIT は 72", () => {
    expect(SUBJECT_LIMIT).toBe(50);
    expect(BODY_WRAP_LIMIT).toBe(72);
  });
});
