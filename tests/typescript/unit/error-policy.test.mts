import { errorMessage, redactSensitiveText } from "fleximark/src/error-policy";
import * as assert from "node:assert/strict";

export const suiteName = "Error redaction policy";
export function suite(): void {
  test("redacts the legacy credential corpus without consuming the next line", () => {
    assert.equal(
      errorMessage(new Error("visible token=abc")),
      "visible token=abc",
    );
    const cases = [
      [
        "token=abc secret value authorization:bearer",
        "token=<redacted> secret <redacted> authorization:<redacted>",
      ],
      [
        "access_token = \"abc def\" client_secret='two words' mytoken\tvalue",
        "access_token = \"<redacted>\" client_secret='<redacted>' mytoken\t<redacted>",
      ],
      [
        "Authorization: Bearer credential\r\nnext line",
        "Authorization: <redacted>\r\nnext line",
      ],
      [
        "authorization=Basic abc def\nauthorization Digest user=x, realm=y, qop=auth\npreauthorization\tAWS4-HMAC-SHA256 Credential=x",
        "authorization=<redacted>\nauthorization <redacted>\npreauthorization\t<redacted>",
      ],
      [
        'Authorization="Basic abc", next=true',
        'Authorization="<redacted>", next=true',
      ],
      [
        '{Authorization: Basic abc,"next":"kept"}',
        '{Authorization: <redacted>,"next":"kept"}',
      ],
      ["authorization   \r\nkept", "authorization   <redacted>\r\nkept"],
      ['{"access_token":"abc"}', '{"access_token":"<redacted>"}'],
      ['{"client_secret":"def"}', '{"client_secret":"<redacted>"}'],
      ['{"mytoken":"ghi"}', '{"mytoken":"<redacted>"}'],
      ['{"Authorization":"Basic abc"}', '{"Authorization":"<redacted>"}'],
      [
        '{"access_token":"abc","client_secret":"def","mytoken":"ghi","Authorization":"Digest xyz"}\r\n{"kept":"next"}',
        '{"access_token":"<redacted>","client_secret":"<redacted>","mytoken":"<redacted>","Authorization":"<redacted>"}\r\n{"kept":"next"}',
      ],
      [
        String.raw`{"access_token":"abc\"def\\ghi"}`,
        '{"access_token":"<redacted>"}',
      ],
      [
        String.raw`{"client_secret":"abc\"def\\ghi"}`,
        '{"client_secret":"<redacted>"}',
      ],
      [String.raw`{"mytoken":"abc\"def\\ghi"}`, '{"mytoken":"<redacted>"}'],
      [
        String.raw`{"Authorization":"Basic abc\"def\\ghi"}`,
        '{"Authorization":"<redacted>"}',
      ],
      [
        '{Authorization: Basic abc, next: kept, "access_token":"xyz", client_secret=raw}',
        '{Authorization: <redacted>, next: kept, "access_token":"<redacted>", client_secret=<redacted>}',
      ],
      [String.raw`{"access_token":"abc\"tail}`, '{"access_token":"<redacted>'],
      [
        `${String.raw`{"Authorization":"Basic abc\\tail}`}\r\nnext=kept`,
        '{"Authorization":"<redacted>\r\nnext=kept',
      ],
      ["İ token=abc", "İ token=<redacted>"],
      ["İaccess_TOKEN=abc", "İaccess_TOKEN=<redacted>"],
      ["İ Authorization: Basic abc", "İ Authorization: <redacted>"],
      ["😀token=abc", "😀token=<redacted>"],
      ["e\u0301 client_secret=def", "e\u0301 client_secret=<redacted>"],
      ["İstanbul 😀 e\u0301 opaque=abc", "İstanbul 😀 e\u0301 opaque=abc"],
    ] as const;
    for (const [input, expected] of cases)
      assert.equal(redactSensitiveText(input), expected);
  });
}
