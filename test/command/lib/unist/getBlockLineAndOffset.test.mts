import * as assert from "assert";
import { Root } from "mdast";

import getBlockLineAndOffset from "../../../../src/command/lib/unist/getBlockLineAndOffset.mjs";

const MdastRootSample1 = {
  type: "root",
  children: [
    {
      type: "heading",
      depth: 1,
      children: [
        {
          type: "text",
          value: "Debug Cursor Highlight",
          position: {
            start: {
              line: 1,
              column: 3,
              offset: 2,
            },
            end: {
              line: 1,
              column: 25,
              offset: 24,
            },
          },
        },
      ],
      position: {
        start: {
          line: 1,
          column: 1,
          offset: 0,
        },
        end: {
          line: 1,
          column: 25,
          offset: 24,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "これはテストです。",
          position: {
            start: {
              line: 3,
              column: 4,
              offset: 29,
            },
            end: {
              line: 3,
              column: 13,
              offset: 38,
            },
          },
        },
      ],
      position: {
        start: {
          line: 3,
          column: 1,
          offset: 26,
        },
        end: {
          line: 3,
          column: 13,
          offset: 38,
        },
      },
    },
    {
      type: "code",
      lang: "html",
      meta: null,
      value:
        '<!DOCTYPE html>\n<html lang="ja">\n<head>\n  <meta charset="UTF-8">\n  <title>カウンター</title>\n  <style>\n    body {\n      font-family: sans-serif;\n      text-align: center;\n      margin-top: 50px;\n    }\n    #count {\n      font-size: 48px;\n      margin: 20px;\n    }\n  </style>\n</head>\n<body>\n  <h1>クリックカウンター</h1>\n  <div id="count">0</div>\n  <button id="incrementBtn">カウントアップ</button>\n\n  <script>\n    const countDisplay = document.getElementById(\'count\');\n    const button = document.getElementById(\'incrementBtn\');\n    let count = 0;\n\n    button.addEventListener(\'click\', () => {\n      count++;\n      countDisplay.textContent = count;\n    });\n  </script>\n</body>\n</html>',
      position: {
        start: {
          line: 5,
          column: 1,
          offset: 40,
        },
        end: {
          line: 40,
          column: 4,
          offset: 714,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "🚀 Overview",
          position: {
            start: {
              line: 42,
              column: 4,
              offset: 719,
            },
            end: {
              line: 42,
              column: 15,
              offset: 730,
            },
          },
        },
      ],
      position: {
        start: {
          line: 42,
          column: 1,
          offset: 716,
        },
        end: {
          line: 42,
          column: 15,
          offset: 730,
        },
      },
    },
    {
      type: "paragraph",
      children: [
        {
          type: "strong",
          children: [
            {
              type: "text",
              value: "FlexiMark",
              position: {
                start: {
                  line: 44,
                  column: 3,
                  offset: 734,
                },
                end: {
                  line: 44,
                  column: 12,
                  offset: 743,
                },
              },
            },
          ],
          position: {
            start: {
              line: 44,
              column: 1,
              offset: 732,
            },
            end: {
              line: 44,
              column: 14,
              offset: 745,
            },
          },
        },
        {
          type: "text",
          value:
            " is a powerful, extensible Markdown toolkit for Visual Studio Code that redefines what Markdown editing and previewing can be.",
          position: {
            start: {
              line: 44,
              column: 14,
              offset: 745,
            },
            end: {
              line: 44,
              column: 140,
              offset: 871,
            },
          },
        },
      ],
      position: {
        start: {
          line: 44,
          column: 1,
          offset: 732,
        },
        end: {
          line: 44,
          column: 140,
          offset: 871,
        },
      },
    },
    {
      type: "paragraph",
      children: [
        {
          type: "text",
          value: "With seamless live preview capabilities both ",
          position: {
            start: {
              line: 46,
              column: 1,
              offset: 873,
            },
            end: {
              line: 46,
              column: 46,
              offset: 918,
            },
          },
        },
        {
          type: "emphasis",
          children: [
            {
              type: "text",
              value: "inside VSCode",
              position: {
                start: {
                  line: 46,
                  column: 47,
                  offset: 919,
                },
                end: {
                  line: 46,
                  column: 60,
                  offset: 932,
                },
              },
            },
          ],
          position: {
            start: {
              line: 46,
              column: 46,
              offset: 918,
            },
            end: {
              line: 46,
              column: 61,
              offset: 933,
            },
          },
        },
        {
          type: "text",
          value: " and ",
          position: {
            start: {
              line: 46,
              column: 61,
              offset: 933,
            },
            end: {
              line: 46,
              column: 66,
              offset: 938,
            },
          },
        },
        {
          type: "emphasis",
          children: [
            {
              type: "text",
              value: "in your web browser",
              position: {
                start: {
                  line: 46,
                  column: 67,
                  offset: 939,
                },
                end: {
                  line: 46,
                  column: 86,
                  offset: 958,
                },
              },
            },
          ],
          position: {
            start: {
              line: 46,
              column: 66,
              offset: 938,
            },
            end: {
              line: 46,
              column: 87,
              offset: 959,
            },
          },
        },
        {
          type: "text",
          value:
            ", FlexiMark gives you complete freedom over customization, interactivity, and style — using the full power of JavaScript.",
          position: {
            start: {
              line: 46,
              column: 87,
              offset: 959,
            },
            end: {
              line: 46,
              column: 208,
              offset: 1080,
            },
          },
        },
      ],
      position: {
        start: {
          line: 46,
          column: 1,
          offset: 873,
        },
        end: {
          line: 46,
          column: 208,
          offset: 1080,
        },
      },
    },
    {
      type: "paragraph",
      children: [
        {
          type: "text",
          value:
            "Whether you're a developer, writer, student, or researcher, FlexiMark is designed to supercharge your Markdown experience — especially for taking structured, rich notes.",
          position: {
            start: {
              line: 48,
              column: 1,
              offset: 1082,
            },
            end: {
              line: 48,
              column: 170,
              offset: 1251,
            },
          },
        },
      ],
      position: {
        start: {
          line: 48,
          column: 1,
          offset: 1082,
        },
        end: {
          line: 48,
          column: 170,
          offset: 1251,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "abc.js test",
          position: {
            start: {
              line: 50,
              column: 4,
              offset: 1256,
            },
            end: {
              line: 50,
              column: 15,
              offset: 1267,
            },
          },
        },
      ],
      position: {
        start: {
          line: 50,
          column: 1,
          offset: 1253,
        },
        end: {
          line: 50,
          column: 15,
          offset: 1267,
        },
      },
    },
    {
      type: "code",
      lang: "abc",
      meta: null,
      value:
        "X:1\nT:Demo Song\nC:Kashiwade\nM:6/8\nL:1/8\nK:Bm\nQ:1/4=175\n%%staves  {(rh) (lh)}\nV:rh clef=treble \nV:lh clef=treble\n% Start of the melody\n[V:rh]\nfefcBe | fcBafe | fefcBe | fcBabf |\n[V:lh]\nG,2D2F2 | G,2D2E2 | G,2D2F2 | G,2D2E2 |\n% next line\n[V:rh]\nedeABd | edeafd | edeABd | [ec]d[fA]B[ec]2 |\n[V:lh]\nF,2A,2C2 | F,2A,2E2 | F,2A,2C2 | F,2A,2E2 |\n%next line\n[V:rh]\n[fB]efcBe | fcBafe | fefcBe | fcBabd' |\n[V:lh]\nG,2D2F2 | G,2D2E2 | G,2D2F2 | G,2D2E2 |\n%next line\n[V:rh]\nc'd'caAf | e2dAc2 | c2d2-dA | c2d4 :|\n[V:lh]\nF,2A,2C2 | F,2C2A2 | B,2D2F2- | FAFEB,2 :|",
      position: {
        start: {
          line: 52,
          column: 1,
          offset: 1269,
        },
        end: {
          line: 83,
          column: 4,
          offset: 1829,
        },
      },
    },
  ],
  position: {
    start: {
      line: 1,
      column: 1,
      offset: 0,
    },
    end: {
      line: 84,
      column: 1,
      offset: 1830,
    },
  },
};

const SourceSample1 = `# Debug Cursor Highlight

## これはテストです。

\`\`\`html
<!DOCTYPE html>
<html lang="ja">
<head>
  <meta charset="UTF-8">
  <title>カウンター</title>
  <style>
    body {
      font-family: sans-serif;
      text-align: center;
      margin-top: 50px;
    }
    #count {
      font-size: 48px;
      margin: 20px;
    }
  </style>
</head>
<body>
  <h1>クリックカウンター</h1>
  <div id="count">0</div>
  <button id="incrementBtn">カウントアップ</button>

  <script>
    const countDisplay = document.getElementById('count');
    const button = document.getElementById('incrementBtn');
    let count = 0;

    button.addEventListener('click', () => {
      count++;
      countDisplay.textContent = count;
    });
  </script>
</body>
</html>
\`\`\`

## 🚀 Overview

**FlexiMark** is a powerful, extensible Markdown toolkit for Visual Studio Code that redefines what Markdown editing and previewing can be.

With seamless live preview capabilities both *inside VSCode* and *in your web browser*, FlexiMark gives you complete freedom over customization, interactivity, and style — using the full power of JavaScript.

Whether you're a developer, writer, student, or researcher, FlexiMark is designed to supercharge your Markdown experience — especially for taking structured, rich notes.

## abc.js test

\`\`\`abc
X:1
T:Demo Song
C:Kashiwade
M:6/8
L:1/8
K:Bm
Q:1/4=175
%%staves  {(rh) (lh)}
V:rh clef=treble 
V:lh clef=treble
% Start of the melody
[V:rh]
fefcBe | fcBafe | fefcBe | fcBabf |
[V:lh]
G,2D2F2 | G,2D2E2 | G,2D2F2 | G,2D2E2 |
% next line
[V:rh]
edeABd | edeafd | edeABd | [ec]d[fA]B[ec]2 |
[V:lh]
F,2A,2C2 | F,2A,2E2 | F,2A,2C2 | F,2A,2E2 |
%next line
[V:rh]
[fB]efcBe | fcBafe | fefcBe | fcBabd' |
[V:lh]
G,2D2F2 | G,2D2E2 | G,2D2F2 | G,2D2E2 |
%next line
[V:rh]
c'd'caAf | e2dAc2 | c2d2-dA | c2d4 :|
[V:lh]
F,2A,2C2 | F,2C2A2 | B,2D2F2- | FAFEB,2 :|
\`\`\`
`;

interface NodeMatchResult {
  type: string;
  language?: string; // For code blocks, e.g. "javascript"
  startLine: number;
  offsetInNode: number;
}

export const suiteName = "commands.utils.getBlockLineAndOffset Utility Tests";
export const suite = () => {
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      6,
      1,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "html",
      startLine: 5,
      offsetInNode: 0,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      6,
      12,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "html",
      startLine: 5,
      offsetInNode: 11,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      13,
      4,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "html",
      startLine: 5,
      offsetInNode: 112,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      24,
      11,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "html",
      startLine: 5,
      offsetInNode: 294,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      39,
      8,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "html",
      startLine: 5,
      offsetInNode: 662,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      54,
      9,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "abc",
      startLine: 52,
      offsetInNode: 12,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      65,
      14,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "abc",
      startLine: 52,
      offsetInNode: 154,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      69,
      7,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "abc",
      startLine: 52,
      offsetInNode: 242,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      70,
      1,
    );
    const expected: NodeMatchResult = {
      type: "code",
      language: "abc",
      startLine: 52,
      offsetInNode: 243,
    };

    assert.deepStrictEqual(ret, expected);
  });
};
