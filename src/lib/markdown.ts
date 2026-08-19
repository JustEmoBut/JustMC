import DOMPurify from "dompurify";
import { marked } from "marked";

/**
 * Render a Modrinth project description.
 *
 * The body is markdown written by whoever published the mod, so it is
 * untrusted input twice over: it can contain raw HTML, and it reaches us over
 * the network. `marked` turns it into HTML and DOMPurify decides what survives
 * — never call one without the other.
 */
export function renderMarkdown(body: string): string {
  const html = marked.parse(body, { async: false, gfm: true, breaks: false });
  return DOMPurify.sanitize(html, {
    // Formatting and links only. No forms, no media, no embedded documents:
    // a mod description has no business asking for input or loading a frame.
    ALLOWED_TAGS: [
      "p", "br", "hr", "strong", "em", "del", "code", "pre", "blockquote",
      "h1", "h2", "h3", "h4", "h5", "h6",
      "ul", "ol", "li", "a", "img",
      "table", "thead", "tbody", "tr", "th", "td",
    ],
    ALLOWED_ATTR: ["href", "src", "alt", "title"],
    ALLOWED_URI_REGEXP: /^https?:\/\//i,
  });
}
