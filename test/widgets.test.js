import { describe, it, expect } from "vitest";

describe("Markdown Decoration Widgets", () => {
  it("Should define properly optimized replacer widgets", async () => {
    const fs = await import("fs");
    const path = await import("path");
    const decorationsSource = fs.readFileSync(
      path.resolve(__dirname, "../src/markdown-decorations.js"),
      "utf-8",
    );

    // 1. Ensure widgets are NOT bypassing native mappings with ignoreEvent false
    expect(decorationsSource).not.toContain("ignoreEvent() { return false; }");

    // 2. Ensure we use view.posAtDOM instead of caching this.pos
    expect(decorationsSource).toContain("view.posAtDOM");
    expect(decorationsSource).not.toContain("this.pos");

    // 3. Ensure we have not reintroduced pos into the MathInlineWidget constructor caching
    expect(decorationsSource).toMatch(
      /class MathInlineWidget extends WidgetType \{\s+constructor\(mathText\)/,
    );
  });
});
