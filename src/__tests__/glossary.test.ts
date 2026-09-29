import { describe, it, expect } from "vitest";
import { GLOSSARY, TERM_KEYS, TERM_PATTERNS } from "../glossary";

describe("glossary", () => {
  it("14 語以上の定義がある", () => {
    expect(TERM_KEYS.length).toBeGreaterThanOrEqual(14);
  });

  it("Issue #207 が求める最低限の用語がそろっている", () => {
    for (const k of [
      "stage", "commit", "branch", "head", "upstream", "push", "pull", "fetch",
      "stash", "merge", "conflict", "fast_forward", "detached_head", "tag",
      "rebase", "remote", "working_tree",
    ]) {
      expect(TERM_KEYS).toContain(k);
    }
  });

  it.each(TERM_KEYS)("%s は全フィールドが埋まっている", (k) => {
    const e = GLOSSARY[k];
    expect(e.label.trim()).not.toBe("");
    expect(e.definition.trim()).not.toBe("");
    expect(e.metaphor.trim()).not.toBe("");
    expect(e.detail.trim()).not.toBe("");
    expect(e.aliases.length).toBeGreaterThan(0);
  });

  it("自動検出パターンは有効な正規表現で、参照先の用語キーが実在する", () => {
    for (const [pattern, key] of TERM_PATTERNS) {
      expect(() => new RegExp(pattern)).not.toThrow();
      expect(GLOSSARY[key]).toBeDefined();
    }
  });
});
