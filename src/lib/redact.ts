/**
 * Strip secrets and personal paths out of game log lines.
 *
 * Minecraft prints `(Session ID is token:<jwt>:<uuid>)` to stdout on startup.
 * Log output is routinely pasted into Discord and issue trackers, so anything
 * leaving this app — the Copy button, and any future upload — must go through
 * here. Redaction happens on the way out rather than on the way in so the live
 * view still shows the user their own unmodified output.
 */
const RULES: [RegExp, string][] = [
  // The session token is the one that can be used to impersonate the account.
  [/\(Session ID is [^)]*\)/gi, "(Session ID is <REDACTED>)"],
  [/--accessToken\s+\S+/gi, "--accessToken <REDACTED>"],
  [/"access_token"\s*:\s*"[^"]*"/gi, '"access_token": "<REDACTED>"'],
  [/"refresh_token"\s*:\s*"[^"]*"/gi, '"refresh_token": "<REDACTED>"'],
  [/"device_code"\s*:\s*"[^"]*"/gi, '"device_code": "<REDACTED>"'],
  // Home directories carry the user's real name on most machines.
  [/([A-Za-z]:\\Users\\)[^\\/\s]+/g, "$1<USER>"],
  [/([A-Za-z]:\/Users\/)[^\\/\s]+/g, "$1<USER>"],
  [/(\/home\/)[^/\s]+/g, "$1<USER>"],
  [/(\/Users\/)[^/\s]+/g, "$1<USER>"],
];

export function redact(text: string): string {
  return RULES.reduce((out, [pattern, replacement]) => out.replace(pattern, replacement), text);
}
