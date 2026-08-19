import { test } from "node:test";
import assert from "node:assert/strict";
import { redact } from "./redact.ts";

test("removes the session token Minecraft prints at startup", () => {
  const line = "[main/INFO]: Setting user: Notch (Session ID is token:eyJhbGciOi.J9:uuid)";
  const out = redact(line);
  assert.ok(!out.includes("eyJhbGciOi"));
  assert.ok(out.includes("(Session ID is <REDACTED>)"));
  assert.ok(out.includes("Setting user: Notch"), "surrounding text must survive");
});

test("removes tokens passed as arguments or printed as json", () => {
  assert.equal(redact("--accessToken abc.def"), "--accessToken <REDACTED>");
  assert.equal(redact('"access_token" : "abc"'), '"access_token": "<REDACTED>"');
  assert.equal(redact('"refresh_token":"abc"'), '"refresh_token": "<REDACTED>"');
  assert.equal(redact('"device_code" : "ABCD"'), '"device_code": "<REDACTED>"');
});

test("masks the account name out of home directory paths", () => {
  assert.equal(
    redact("at C:\\Users\\Just\\AppData\\Roaming\\JustLauncher"),
    "at C:\\Users\\<USER>\\AppData\\Roaming\\JustLauncher"
  );
  assert.equal(redact("/home/just/.local/share"), "/home/<USER>/.local/share");
  assert.equal(redact("/Users/just/Library"), "/Users/<USER>/Library");
});

test("leaves ordinary log lines untouched", () => {
  const line = "[12:41:41] [Client thread/INFO]: Created: 512x512 textures-atlas";
  assert.equal(redact(line), line);
});

test("redacts every occurrence, not just the first", () => {
  const out = redact("(Session ID is token:a) then (Session ID is token:b)");
  assert.ok(!out.includes("token:"), out);
});
