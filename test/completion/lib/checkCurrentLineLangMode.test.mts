import assert from "assert";

import checkCurrentLineLangMode from "../../../src/completion/lib/checkCurrentLineLangMode.mjs";

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

export const suiteName = "commands.utils.getBlockLineAndOffset Utility Tests";
export const suite = () => {
  test("checkCurrentLineLangMode() should return basic result (part1)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 5);
    const expected = "html";

    assert.deepStrictEqual(ret, expected);
  });
  test("checkCurrentLineLangMode() should return basic result (part2)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 1);
    const expected = undefined;

    assert.deepStrictEqual(ret, expected);
  });
  test("checkCurrentLineLangMode() should return basic result (part3)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 18);
    const expected = "html";

    assert.deepStrictEqual(ret, expected);
  });
  test("checkCurrentLineLangMode() should return basic result (part4)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 58);
    const expected = "abc";

    assert.deepStrictEqual(ret, expected);
  });
  test("checkCurrentLineLangMode() should return basic result (part5)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 83);
    const expected = "abc";

    assert.deepStrictEqual(ret, expected);
  });
  test("checkCurrentLineLangMode() should return basic result (part6)", () => {
    const ret = checkCurrentLineLangMode(SourceSample1, 84);
    const expected = undefined;

    assert.deepStrictEqual(ret, expected);
  });
};
