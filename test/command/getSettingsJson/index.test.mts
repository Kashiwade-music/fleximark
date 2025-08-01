/* eslint-disable @typescript-eslint/no-explicit-any */
import * as assert from "assert";

const { sortObjectKeys, addCommentsToJson, objectToJsonLines, KeyOrder } = (
  globalThis as any
).commands.genSettingsJson;

export const suiteName = "genSettingsJson Utility Tests";

export const suite = () => {
  // sortObjectKeys
  test("sortObjectKeys() should sort keys based on KeyOrder and then alphabetically", () => {
    const input = {
      "fleximark.noteFileNameSuffix": "suffix",
      zzz: 1,
      aaa: 2,
      "fleximark.noteTemplates": "template",
      "fleximark.noteFileNamePrefix": "prefix",
    };
    const expectedOrder = [
      "fleximark.noteFileNamePrefix",
      "fleximark.noteFileNameSuffix",
      "fleximark.noteTemplates",
      "aaa",
      "zzz",
    ];

    const result = sortObjectKeys(input);
    assert.deepStrictEqual(Object.keys(result), expectedOrder);
  });

  test("sortObjectKeys() should return an empty object when input is empty", () => {
    const input = {};
    const result = sortObjectKeys(input);
    assert.deepStrictEqual(result, {});
  });

  test("sortObjectKeys() should preserve KeyOrder even if some keys are missing", () => {
    const input = {
      "fleximark.settingsVersion": "1.0",
      "fleximark.noteTemplates": "template",
      "markdown.copyFiles.destination": "dest",
    };
    const expectedOrder = [
      "fleximark.settingsVersion",
      "fleximark.noteTemplates",
      "markdown.copyFiles.destination",
    ];
    const result = sortObjectKeys(input);
    assert.deepStrictEqual(Object.keys(result), expectedOrder);
  });

  test("sortObjectKeys() should append unknown keys in alphabetical order", () => {
    const input = {
      zzz: "last",
      bbb: "second",
      aaa: "first",
    };
    const expectedOrder = ["aaa", "bbb", "zzz"];
    const result = sortObjectKeys(input);
    assert.deepStrictEqual(Object.keys(result), expectedOrder);
  });

  test("sortObjectKeys() should retain values correctly after sorting", () => {
    const input = {
      "fleximark.noteFileNamePrefix": "pre",
      "fleximark.noteFileNameSuffix": "suf",
      xyz: 42,
    };
    const expected = {
      "fleximark.noteFileNamePrefix": "pre",
      "fleximark.noteFileNameSuffix": "suf",
      xyz: 42,
    };
    const result = sortObjectKeys(input);
    assert.deepStrictEqual(result, expected);
  });

  test("sortObjectKeys() should handle all KeyOrder keys in correct order", () => {
    const input = Object.fromEntries(
      KeyOrder.map((key: any, i: any) => [key, i]),
    );
    const result = sortObjectKeys(input);
    assert.deepStrictEqual(Object.keys(result), KeyOrder);
  });

  // addCommentsToJson
  test("addCommentsToJson() should insert comments for matching keys", () => {
    const jsonLines = [
      "{",
      '  "fleximark.noteFileNamePrefix": "prefix",',
      '  "zzz": 123',
      "}",
    ];
    const comments = {
      "fleximark.noteFileNamePrefix": ["This is a prefix"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "  // This is a prefix",
      '  "fleximark.noteFileNamePrefix": "prefix",',
      '  "zzz": 123',
      "}",
    ]);
  });

  test("addCommentsToJson() should insert blank line before subsequent comment groups", () => {
    const jsonLines = ["{", '  "key1": "value1",', '  "key2": "value2"', "}"];
    const comments = {
      key1: ["Comment for key1"],
      key2: ["Comment for key2"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "  // Comment for key1",
      '  "key1": "value1",',
      "",
      "  // Comment for key2",
      '  "key2": "value2"',
      "}",
    ]);
  });

  test("addCommentsToJson() should not modify lines when no keys match", () => {
    const jsonLines = ["{", '  "foo": "bar",', '  "baz": 42', "}"];
    const comments = {
      nonexistent: ["No match"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      '  "foo": "bar",',
      '  "baz": 42',
      "}",
    ]);
  });

  test("addCommentsToJson() should add multiple comment lines per key", () => {
    const jsonLines = ["{", '  "key": true', "}"];
    const comments = {
      key: ["First line", "Second line", "Third line"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "  // First line",
      "  // Second line",
      "  // Third line",
      '  "key": true',
      "}",
    ]);
  });

  test("addCommentsToJson() should handle keys with different indentation", () => {
    const jsonLines = [
      "{",
      '    "deepKey": "value",',
      '  "shallowKey": 1',
      "}",
    ];
    const comments = {
      deepKey: ["Deeply nested"],
      shallowKey: ["Top level"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "    // Deeply nested",
      '    "deepKey": "value",',
      "",
      "  // Top level",
      '  "shallowKey": 1',
      "}",
    ]);
  });

  test("addCommentsToJson() should preserve original order of keys", () => {
    const jsonLines = ["{", '  "b": 2,', '  "a": 1', "}"];
    const comments = {
      a: ["Comment for a"],
      b: ["Comment for b"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "  // Comment for b",
      '  "b": 2,',
      "",
      "  // Comment for a",
      '  "a": 1',
      "}",
    ]);
  });

  test("addCommentsToJson() should handle keys with special characters", () => {
    const jsonLines = ["{", '  "a.b-c_d": "value"', "}"];
    const comments = {
      "a.b-c_d": ["Special key"],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, [
      "{",
      "  // Special key",
      '  "a.b-c_d": "value"',
      "}",
    ]);
  });

  test("addCommentsToJson() should handle empty comment arrays", () => {
    const jsonLines = ["{", '  "foo": 1', "}"];
    const comments = {
      foo: [],
    };

    const result = addCommentsToJson(jsonLines, comments);

    assert.deepStrictEqual(result, ["{", '  "foo": 1', "}"]);
  });

  // objectToJsonLines
  test("objectToJsonLines() should handle empty object", () => {
    const obj = {};
    const result = objectToJsonLines(obj);
    assert.deepStrictEqual(result, ["{}"]);
  });

  test("objectToJsonLines() should handle nested objects", () => {
    const obj = {
      a: 1,
      b: {
        c: 2,
        d: 3,
      },
    };
    const result = objectToJsonLines(obj);
    assert.deepStrictEqual(result, [
      "{",
      '  "a": 1,',
      '  "b": {',
      '    "c": 2,',
      '    "d": 3',
      "  }",
      "}",
    ]);
  });

  test("objectToJsonLines() should handle arrays in object", () => {
    const obj = {
      items: [1, 2, 3],
    };
    const result = objectToJsonLines(obj);
    assert.deepStrictEqual(result, [
      "{",
      '  "items": [',
      "    1,",
      "    2,",
      "    3",
      "  ]",
      "}",
    ]);
  });

  test("objectToJsonLines() should handle string values", () => {
    const obj = {
      message: "hello",
    };
    const result = objectToJsonLines(obj);
    assert.deepStrictEqual(result, ["{", '  "message": "hello"', "}"]);
  });

  test("objectToJsonLines() should handle boolean and null values", () => {
    const obj = {
      isActive: true,
      isDeleted: false,
      data: null,
    };
    const result = objectToJsonLines(obj);
    assert.deepStrictEqual(result, [
      "{",
      '  "isActive": true,',
      '  "isDeleted": false,',
      '  "data": null',
      "}",
    ]);
  });
};
