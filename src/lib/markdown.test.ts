import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";

// DOMPurify needs a DOM; node has none. Install one before importing the
// module under test, which binds to whatever `window` exists at import time.
const dom = new JSDOM("");
(globalThis as unknown as { window: unknown }).window = dom.window;
(globalThis as unknown as { document: unknown }).document = dom.window.document;

const { renderMarkdown } = await import("./markdown.ts");

test("renders the formatting a project description actually uses", () => {
  const html = renderMarkdown("# Sodium\n\nA **fast** renderer.\n\n- one\n- two\n");
  assert.match(html, /<h1>Sodium<\/h1>/);
  assert.match(html, /<strong>fast<\/strong>/);
  assert.match(html, /<li>one<\/li>/);
});

test("strips script, event handlers and frames", () => {
  const html = renderMarkdown(
    ["<script>alert(1)</script>", '<img src="x" onerror="alert(1)">', '<iframe src="https://evil.test"></iframe>'].join("\n\n")
  );
  assert.doesNotMatch(html, /<script/i);
  assert.doesNotMatch(html, /onerror/i);
  assert.doesNotMatch(html, /<iframe/i);
});

test("drops a link whose href is not http(s)", () => {
  // The text may survive; the navigation must not.
  const html = renderMarkdown("[click](javascript:alert(1))\n\n[f](file:///etc/passwd)");
  assert.doesNotMatch(html, /href="javascript/i);
  assert.doesNotMatch(html, /href="file/i);
});

test("keeps ordinary links and images", () => {
  const html = renderMarkdown("[docs](https://modrinth.com) ![shot](https://cdn.modrinth.com/a.png)");
  assert.match(html, /href="https:\/\/modrinth\.com"/);
  assert.match(html, /src="https:\/\/cdn\.modrinth\.com\/a\.png"/);
});
