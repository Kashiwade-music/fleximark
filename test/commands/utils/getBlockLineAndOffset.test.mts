import * as assert from "assert";
import { Root } from "mdast";

import getBlockLineAndOffset from "../../../src/commands/utils/getBlockLineAndOffset.mjs";

const MdastRootSample1 = {
  type: "root",
  children: [
    {
      type: "heading",
      depth: 1,
      children: [
        {
          type: "text",
          value: "原神 龍に選ばれし者の試練",
          position: {
            start: {
              line: 1,
              column: 3,
              offset: 2,
            },
            end: {
              line: 1,
              column: 16,
              offset: 15,
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
          column: 16,
          offset: 15,
        },
      },
    },
    {
      type: "paragraph",
      children: [
        {
          type: "text",
          value: "Note Created: 2025-07-20",
          position: {
            start: {
              line: 2,
              column: 1,
              offset: 16,
            },
            end: {
              line: 2,
              column: 25,
              offset: 40,
            },
          },
        },
      ],
      position: {
        start: {
          line: 2,
          column: 1,
          offset: 16,
        },
        end: {
          line: 2,
          column: 25,
          offset: 40,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "原曲",
          position: {
            start: {
              line: 4,
              column: 4,
              offset: 45,
            },
            end: {
              line: 4,
              column: 6,
              offset: 47,
            },
          },
        },
      ],
      position: {
        start: {
          line: 4,
          column: 1,
          offset: 42,
        },
        end: {
          line: 4,
          column: 6,
          offset: 47,
        },
      },
    },
    {
      type: "html",
      value:
        '<div class="youtube-placeholder" data-video-id="cUo2ljhBn0A" style="position: relative; width: 100%; padding-bottom: 56.25%; cursor: pointer;">\n      <img src="https://img.youtube.com/vi/cUo2ljhBn0A/hqdefault.jpg" style="position: absolute; top: 0; left: 0; width: 100%; height: 100%; object-fit: cover;">\n      <div style="position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); width: 68px; height: 48px;">\n        <svg viewBox="0 0 68 48" width="100%" height="100%">\n          <path d="M66.52 7.01a8 8 0 0 0-5.63-5.66C56.23 0.4 34 0.4 34 0.4s-22.23 0-26.89.95A8 8 0 0 0 1.48 7.01 83.16 83.16 0 0 0 0 24a83.16 83.16 0 0 0 1.48 16.99 8 8 0 0 0 5.63 5.66c4.66.95 26.89.95 26.89.95s22.23 0 26.89-.95a8 8 0 0 0 5.63-5.66A83.16 83.16 0 0 0 68 24a83.16 83.16 0 0 0-1.48-16.99z" fill="#f00"/>\n          <path d="M45 24 27 14v20z" fill="#fff"/>\n        </svg>\n      </div>\n    </div>',
      data: {
        hName: "div",
        hProperties: {
          className: "embed-content youtube-embed",
        },
      },
      position: {
        start: {
          line: 6,
          column: 1,
          offset: 49,
        },
        end: {
          line: 6,
          column: 44,
          offset: 92,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "イントロ",
          position: {
            start: {
              line: 8,
              column: 4,
              offset: 97,
            },
            end: {
              line: 8,
              column: 8,
              offset: 101,
            },
          },
        },
      ],
      position: {
        start: {
          line: 8,
          column: 1,
          offset: 94,
        },
        end: {
          line: 8,
          column: 8,
          offset: 101,
        },
      },
    },
    {
      type: "code",
      lang: "abc",
      meta: null,
      value:
        'X:1\nT:Intro\nM:4/4\nL:1/8\nK:Am\nQ:156\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name="Melody" snm="M"\n%%MIDI program 80\nV:rh clef=treble name="Chord" snm="C"\n%%MIDI program 0\nV:lh clef=bass  octave=-2\n% Start of the melody\n[V:ml]\nGAEG ABDE | GEG2 [A,E][CG][DA][EB]| GAEG ABDE | GEG2 [A,E][CG][DA][Ec]|\n[V:rh]\n[A,DEG]8- | [A,DEG]8 | [G,DEA]8- | [G,DEA]8 |\n[V:lh]\nA8- | A8 | C8- | C8 |\n% 2-line\n[V:ml]\nGAEG ABDE | GEG2 [A,E][CG][DA][EB]| GAEG ABDE | GEG2 [A,E][CG][DA][EB]||\n[V:rh]\n[EGAc]8- | [EGAc]8 | [DEGB]8- | [DEGB]8 ||\n[V:lh]\nF8- | F8 | G8- | G4 E4 ||\n% next line\n[V:ml]\nGAEG ABDE | GEGA DEAB | GAEG ABDE | GEGA DEAB |\n[V:rh]\n[A,EA]8- | [A,EA]8 | [G,DA]8- | [G,DA]8 |\n[V:lh]\nA8- | A4 A2A2 | G8- | G4 G2G2\n% next line\n[V:ml]\nGAEG ABDE | GEGA DEAB | GAEG ABDE | GEGA DEAB |\n[V:rh]\n[A,EA]8- | [A,EA]8 | [A,B,D]8- | [A,B,D]4 [^G,B,E]4 |\n[V:lh]\nF8- | F4 F2F2 | E8- | E4 E2E2 |',
      position: {
        start: {
          line: 10,
          column: 1,
          offset: 103,
        },
        end: {
          line: 51,
          column: 4,
          offset: 987,
        },
      },
    },
    {
      type: "heading",
      depth: 2,
      children: [
        {
          type: "text",
          value: "Aメロ",
          position: {
            start: {
              line: 53,
              column: 4,
              offset: 992,
            },
            end: {
              line: 53,
              column: 7,
              offset: 995,
            },
          },
        },
      ],
      position: {
        start: {
          line: 53,
          column: 1,
          offset: 989,
        },
        end: {
          line: 53,
          column: 7,
          offset: 995,
        },
      },
    },
    {
      type: "code",
      lang: "abc",
      meta: null,
      value:
        "X:1\nT:Aメロ\nM:4/4\nL:1/8\nK:Am\nQ:156\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name=\"Melody\" snm=\"M\"\n%%MIDI program 80\nV:rh clef=treble name=\"Chord\" snm=\"C\"\n%%MIDI program 0\nV:lh clef=bass  octave=-2\n% Start of the melody\n[V:ml]\n^f6 gf- | f3b-b2^f2 | g^fec-c4- | c6 (5 ABcde |\n[V:rh]\n[A,B,CE^F]8| [A,B,CE^F]8 | [^F,A,DE]8 | [^F,A,DE]8 |\n[V:lh]\nA8 | A8 | D8 | D8 |\n% next line\n[V:ml]\n^f6 gf-    | ^f3b-b2^f2  | g^fdd'-d'4- | d'6 g/a/_b/c'/ |\n[V:rh]\n[A,B,CE^F]8| [A,B,CE^F]8 | [_B,_EG]8   | [_B,_EG]8      |\n[V:lh]\nA8         | A8          | C8          | C8             |\n% next line\n[V:ml]\nd'6 e'd'- | d'3f'-f'2d'2 | _e'd'c'_a-a4- | a6 c'2 |\n[V:rh]\n[_B,DF]8| [_B,DF]8 | [_A,C_E]8 | [_A,C_E]8 |\n[V:lh]\n_B8 | _B8 | _A8 | _A8 |\n% next line\n[V:ml]\n_d'c'_b_g-g4-  | g4 _b4 | c'8- | c'8 |\n[V:rh]\n[_B,_D_G]8| [_B,_D_G]8 | [CEG]8 | [CEG]8 |\n[V:lh]\n_G8 | _G8 | C8 | C8 |",
      position: {
        start: {
          line: 55,
          column: 1,
          offset: 997,
        },
        end: {
          line: 96,
          column: 4,
          offset: 1866,
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
      line: 96,
      column: 4,
      offset: 1866,
    },
  },
};

const SourceSample1 = `# 原神 龍に選ばれし者の試練
Note Created: 2025-07-20

## 原曲

https://www.youtube.com/watch?v=cUo2ljhBn0A

## イントロ

\`\`\`abc
X:1
T:Intro
M:4/4
L:1/8
K:Am
Q:156
%%staves (ml) {(rh) (lh)}
V:ml clef=treble name="Melody" snm="M"
%%MIDI program 80
V:rh clef=treble name="Chord" snm="C"
%%MIDI program 0
V:lh clef=bass  octave=-2
% Start of the melody
[V:ml]
GAEG ABDE | GEG2 [A,E][CG][DA][EB]| GAEG ABDE | GEG2 [A,E][CG][DA][Ec]|
[V:rh]
[A,DEG]8- | [A,DEG]8 | [G,DEA]8- | [G,DEA]8 |
[V:lh]
A8- | A8 | C8- | C8 |
% 2-line
[V:ml]
GAEG ABDE | GEG2 [A,E][CG][DA][EB]| GAEG ABDE | GEG2 [A,E][CG][DA][EB]||
[V:rh]
[EGAc]8- | [EGAc]8 | [DEGB]8- | [DEGB]8 ||
[V:lh]
F8- | F8 | G8- | G4 E4 ||
% next line
[V:ml]
GAEG ABDE | GEGA DEAB | GAEG ABDE | GEGA DEAB |
[V:rh]
[A,EA]8- | [A,EA]8 | [G,DA]8- | [G,DA]8 |
[V:lh]
A8- | A4 A2A2 | G8- | G4 G2G2
% next line
[V:ml]
GAEG ABDE | GEGA DEAB | GAEG ABDE | GEGA DEAB |
[V:rh]
[A,EA]8- | [A,EA]8 | [A,B,D]8- | [A,B,D]4 [^G,B,E]4 |
[V:lh]
F8- | F4 F2F2 | E8- | E4 E2E2 |
\`\`\`

## Aメロ

\`\`\`abc
X:1
T:Aメロ
M:4/4
L:1/8
K:Am
Q:156
%%staves (ml) {(rh) (lh)}
V:ml clef=treble name="Melody" snm="M"
%%MIDI program 80
V:rh clef=treble name="Chord" snm="C"
%%MIDI program 0
V:lh clef=bass  octave=-2
% Start of the melody
[V:ml]
^f6 gf- | f3b-b2^f2 | g^fec-c4- | c6 (5 ABcde |
[V:rh]
[A,B,CE^F]8| [A,B,CE^F]8 | [^F,A,DE]8 | [^F,A,DE]8 |
[V:lh]
A8 | A8 | D8 | D8 |
% next line
[V:ml]
^f6 gf-    | ^f3b-b2^f2  | g^fdd'-d'4- | d'6 g/a/_b/c'/ |
[V:rh]
[A,B,CE^F]8| [A,B,CE^F]8 | [_B,_EG]8   | [_B,_EG]8      |
[V:lh]
A8         | A8          | C8          | C8             |
% next line
[V:ml]
d'6 e'd'- | d'3f'-f'2d'2 | _e'd'c'_a-a4- | a6 c'2 |
[V:rh]
[_B,DF]8| [_B,DF]8 | [_A,C_E]8 | [_A,C_E]8 |
[V:lh]
_B8 | _B8 | _A8 | _A8 |
% next line
[V:ml]
_d'c'_b_g-g4-  | g4 _b4 | c'8- | c'8 |
[V:rh]
[_B,_D_G]8| [_B,_D_G]8 | [CEG]8 | [CEG]8 |
[V:lh]
_G8 | _G8 | C8 | C8 |
\`\`\``;

interface NodeMatchResult {
  type: string;
  startLine: number;
  offsetInNode: number;
}

export const suiteName = "commands.utils.getBlockLineAndOffset Utility Tests";
export const suite = () => {
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      1,
      1,
    );
    const expected: NodeMatchResult = {
      type: "heading",
      startLine: 1,
      offsetInNode: 0,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      6,
      1,
    );
    const expected: NodeMatchResult = {
      type: "html",
      startLine: 6,
      offsetInNode: 0,
    };

    assert.deepStrictEqual(ret, expected);
  });
  test("getBlockLineAndOffset() should return basic result", () => {
    const ret = getBlockLineAndOffset(
      MdastRootSample1 as Root,
      SourceSample1,
      6,
      21,
    );
    const expected: NodeMatchResult = {
      type: "html",
      startLine: 6,
      offsetInNode: 20,
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
      startLine: 10,
      offsetInNode: 15,
    };

    assert.deepStrictEqual(ret, expected);
  });
};
