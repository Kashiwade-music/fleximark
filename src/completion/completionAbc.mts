import * as vscode from "vscode";

import checkCurrentLineLangMode from "./lib/checkCurrentLineLangMode.mjs";

export const slashCommand = vscode.languages.registerCompletionItemProvider(
  "markdown",
  {
    provideCompletionItems(
      document: vscode.TextDocument,
      position: vscode.Position,
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      token: vscode.CancellationToken,
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      context: vscode.CompletionContext,
    ) {
      const currentLangMode = checkCurrentLineLangMode(
        document.getText(),
        position.line + 1, // Convert to 1-based line number
      );

      if (currentLangMode !== "abc") {
        return []; // Return empty array if not in ABC mode
      }

      //今カーソルのいる位置の文字が \ であることを確認
      const currentChar = document.getText(
        new vscode.Range(position.translate(0, -1), position),
      );
      if (currentChar !== "\\") {
        return []; // Return empty array if the character before the cursor is not '\'
      }

      // ===================================

      const octaveCompletion = new vscode.CompletionItem("\\noteOctave");
      octaveCompletion.kind = vscode.CompletionItemKind.Function;
      octaveCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      octaveCompletion.insertText = new vscode.SnippetString(
        "${1|'',',\\,,\\,\\,|}",
      );
      octaveCompletion.documentation = new vscode.MarkdownString(
        `Insert octave changes.
- e.g.
  - \`'\`: one octave up
  - \`,\`: one octave down
  - \`''\`: two octaves up
  - \`,,\`: three octaves down`,
      );

      // ===================================

      const accidentalsCompletion = new vscode.CompletionItem(
        "\\noteAccidentals",
      );
      accidentalsCompletion.kind = vscode.CompletionItemKind.Function;
      accidentalsCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      accidentalsCompletion.insertText = new vscode.SnippetString(
        "${1|^,=,_,^^,__|}",
      );
      accidentalsCompletion.documentation = new vscode.MarkdownString(
        `Insert accidentals.
- e.g.
  - \`^c\`: sharp
  - \`=c\`: natural
  - \`_c\`: flat
  - \`^^c\`: double sharp
  - \`__c\`: double flat`,
      );

      // ===================================

      const shorterLengthsCompletion = new vscode.CompletionItem(
        "\\noteShorterLengths",
      );
      shorterLengthsCompletion.kind = vscode.CompletionItemKind.Function;
      shorterLengthsCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      shorterLengthsCompletion.insertText = new vscode.SnippetString(
        "${1|3/4,/,/4|}",
      );

      shorterLengthsCompletion.documentation = new vscode.MarkdownString(
        `Insert shorter lengths.
- e.g.
  - \`3/4\`: three-quarters length
  - \`/\`: half length
  - \`/4\`: quarter length`,
      );

      // ===================================

      const longerLengthsCompletion = new vscode.CompletionItem(
        "\\noteLongerLengths",
      );
      longerLengthsCompletion.kind = vscode.CompletionItemKind.Function;
      longerLengthsCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      longerLengthsCompletion.insertText = new vscode.SnippetString(
        "${1|3/2,2,3,4|}",
      );
      longerLengthsCompletion.documentation = new vscode.MarkdownString(
        `Insert longer lengths.
- e.g.
  - \`3/2\`: three-halves length
  - \`2\`: double length
  - \`3\`: triple length
  - \`4\`: quadruple length`,
      );

      // ===================================

      const barLinesCompletion = new vscode.CompletionItem("\\barlines");
      barLinesCompletion.kind = vscode.CompletionItemKind.Function;
      barLinesCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      barLinesCompletion.insertText = new vscode.SnippetString(
        "${1|\\|,\\|],\\|\\|,[\\|,\\|:,:\\|,::,\\|1,:\\|2|}",
      );
      barLinesCompletion.documentation = new vscode.MarkdownString(
        `* \`|\`: bar line
* \`|]\`: thin-thick double bar line
* \`||\`: thin-thin double bar line
* \`[|\`: thick-thin double bar line
* \`|:\`: start of repeated section
* \`:|\`: end of repeated section
* \`::\`: start & end of two repeated sections
* \`|1\`: start of first ending
* \`:|2\`: start of second ending`,
      );

      // ===================================

      const tiesCompletion = new vscode.CompletionItem("\\ties");
      tiesCompletion.kind = vscode.CompletionItemKind.Function;
      tiesCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      tiesCompletion.insertText = "-";
      tiesCompletion.documentation = new vscode.MarkdownString(
        `Insert a tie (dash) between notes.
- e.g.
  - \`abc-|cba\`
  - \`c4-c4\``,
      );

      // ===================================

      const slursCompletion = new vscode.CompletionItem("\\slurs");
      slursCompletion.kind = vscode.CompletionItemKind.Function;
      slursCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      slursCompletion.insertText = new vscode.SnippetString("($0)");
      slursCompletion.documentation = new vscode.MarkdownString(
        `Insert a slur (parentheses) around notes.
- e.g.
  - \`(c (d e f) g a)\`: Slurs may be nested
  - \`(c d (e) f g a)\`: Slurs may also start and end on the same note`,
      );

      // ===================================

      const graceNotesCompletion = new vscode.CompletionItem("\\gracenotes");
      graceNotesCompletion.kind = vscode.CompletionItemKind.Function;
      graceNotesCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      graceNotesCompletion.insertText = new vscode.SnippetString(
        "${1|{/CDE},{CDE}|}",
      );
      graceNotesCompletion.documentation = new vscode.MarkdownString(
        `Insert grace notes.
- e.g.
  - \`{/CDE}\`: Grace notes with slashes.
  - \`{CDE}\`: Grace notes before the main note and after`,
      );

      // ===================================

      const tripletsCompletion = new vscode.CompletionItem("\\triplets");
      tripletsCompletion.kind = vscode.CompletionItemKind.Function;
      tripletsCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      tripletsCompletion.insertText = new vscode.SnippetString(
        "${1|(3,(5,(p:q:r|}",
      );
      tripletsCompletion.documentation =
        new vscode.MarkdownString(`Insert triplets.
- e.g.
  - \`(3 CDE\`: Simple triplet
  - \`(p:q:r \`: put *p* notes into the time of *q* for the next *r* notes`);

      // ===================================

      const decorationCompletion = new vscode.CompletionItem("\\decorations");
      decorationCompletion.kind = vscode.CompletionItemKind.Function;
      decorationCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      decorationCompletion.insertText = "!";
      decorationCompletion.documentation = new vscode.MarkdownString(
        "Insert ABC decoration marks.",
      );
      decorationCompletion.command = {
        command: "editor.action.triggerSuggest",
        title: "Trigger Suggest",
      };

      // ===================================

      const multipleNotesCompletion = new vscode.CompletionItem(
        "\\multipleNotes",
      );
      multipleNotesCompletion.kind = vscode.CompletionItemKind.Function;
      multipleNotesCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      multipleNotesCompletion.insertText = new vscode.SnippetString(
        "[${1:CEG}]",
      );
      multipleNotesCompletion.documentation = new vscode.MarkdownString(
        "Insert multiple notes.",
      );

      // ===================================

      const chordCompletion = new vscode.CompletionItem("\\chord");
      chordCompletion.kind = vscode.CompletionItemKind.Function;
      chordCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      chordCompletion.insertText = new vscode.SnippetString('"$0"');
      chordCompletion.documentation = new vscode.MarkdownString(
        "Insert a chord symbol. You can use \\chordAccidental and \\chordType",
      );

      // ===================================

      const chordAccidentalCompletion = new vscode.CompletionItem(
        "\\chordAccidental",
      );
      chordAccidentalCompletion.kind = vscode.CompletionItemKind.Function;
      chordAccidentalCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      chordAccidentalCompletion.insertText = new vscode.SnippetString(
        "${1|#,b|}",
      );
      chordAccidentalCompletion.documentation = new vscode.MarkdownString(
        "Insert a chord accidental. Use # for sharp and b for flat.",
      );

      // ===================================

      const chordTypeCompletion = new vscode.CompletionItem("\\chordType");
      chordTypeCompletion.kind = vscode.CompletionItemKind.Function;
      chordTypeCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      chordTypeCompletion.insertText = new vscode.SnippetString(
        "${1|maj7,6,7,+,aug7,m,m6,m7,dim,dim7,ø7,9,11,13,7b9,7b5,9+5,sus4,7sus4,m7sus4|}",
      );
      chordTypeCompletion.documentation = new vscode.MarkdownString(
        "Insert a chord type.",
      );

      // ===================================

      const newLineCompletion = new vscode.CompletionItem("\\newline");
      newLineCompletion.kind = vscode.CompletionItemKind.Function;
      newLineCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      newLineCompletion.insertText = new vscode.SnippetString("$0| | | |");
      newLineCompletion.documentation = new vscode.MarkdownString(
        "Insert a new line in ABC notation.",
      );

      // return all completion items as array
      return [
        octaveCompletion,
        accidentalsCompletion,
        shorterLengthsCompletion,
        longerLengthsCompletion,
        barLinesCompletion,
        tiesCompletion,
        slursCompletion,
        graceNotesCompletion,
        tripletsCompletion,
        decorationCompletion,
        multipleNotesCompletion,
        chordCompletion,
        chordAccidentalCompletion,
        chordTypeCompletion,
        newLineCompletion,
      ];
    },
  },
  "\\",
);

export const decorations = vscode.languages.registerCompletionItemProvider(
  "markdown",
  {
    provideCompletionItems(
      document: vscode.TextDocument,
      position: vscode.Position,
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      token: vscode.CancellationToken,
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      context: vscode.CompletionContext,
    ) {
      const currentLangMode = checkCurrentLineLangMode(
        document.getText(),
        position.line + 1, // Convert to 1-based line number
      );

      if (currentLangMode !== "abc") {
        return []; // Return empty array if not in ABC mode
      }

      //今カーソルのいる位置の文字が ! であることを確認
      const currentChar = document.getText(
        new vscode.Range(position.translate(0, -1), position),
      );
      if (currentChar !== "!") {
        return []; // Return empty array if the character before the cursor is not '!'
      }

      // ===================================

      const abcDecorations: Record<string, string> = {
        "!mark!": "a highlight mark",
        "!trill!": "tr (trill mark)",
        "!trill(! $0 !trill)!": "an extended trill",
        "!lowermordent!":
          "short /|/|/ squiggle with a vertical line through it",
        "!uppermordent!": "short /|/|/ squiggle",
        "!mordent!": "same as !lowermordent!",
        "!pralltriller!": "same as !uppermordent!",
        "!roll!": "a roll mark (arc) as used in Irish music",
        "!turn!": "a turn mark (also known as gruppetto)",
        "!turnx!": "a turn mark with a line through it",
        "!invertedturn!": "an inverted turn mark",
        "!invertedturnx!": "an inverted turn mark with a line through it",
        "!arpeggio!": "vertical squiggle",
        "!>!": "> mark",
        "!accent!": "same as !>!",
        "!emphasis!": "same as !>!",
        "!fermata!": "fermata or hold (arc above dot)",
        "!invertedfermata!": "upside down fermata",
        "!tenuto!":
          "horizontal line to indicate holding note for full duration",
        "!0!": "fingering 0",
        "!1!": "fingering 1",
        "!2!": "fingering 2",
        "!3!": "fingering 3",
        "!4!": "fingering 4",
        "!5!": "fingering 5",
        "!+!": "left-hand pizzicato, or rasp for French horns",
        "!plus!": "same as !+!",
        "!snap!": "snap-pizzicato mark, visually similar to !thumb!",
        "!slide!": "slide up to a note, visually similar to a half slur",
        "!wedge!": "small filled-in wedge mark",
        "!upbow!": "V mark",
        "!downbow!": "squared n mark",
        "!open!": "small circle above note indicating open string or harmonic",
        "!thumb!": "cello thumb symbol",
        "!breath!": "a breath mark (apostrophe-like) after note",
        "!pppp!": "dynamics mark",
        "!ppp!": "dynamics mark",
        "!pp!": "dynamics mark",
        "!p!": "dynamics mark",
        "!mp!": "dynamics mark",
        "!mf!": "dynamics mark",
        "!f!": "dynamics mark",
        "!ff!": "dynamics mark",
        "!fff!": "dynamics mark",
        "!ffff!": "dynamics mark",
        "!sfz!": "dynamics mark",
        "!crescendo(! $0 !crescendo)!": "crescendo mark",
        "!<(! $0! <)!": "same as !crescendo(!",
        "!diminuendo(! $0 !diminuendo)!": "diminuendo mark",
        "!>(! $0! >)!": "same as !diminuendo(!",
        "!segno!": "2 ornate s-like symbols separated by a diagonal line",
        "!coda!": "a ring with a cross in it",
        "!D.S.!": "the letters D.S. (=Da Segno)",
        "!D.C.!": "the letters D.C. (=either Da Coda or Da Capo)",
        "!dacoda!": 'the word "Da" followed by a Coda sign',
        "!dacapo!": 'the words "Da Capo"',
        "!fine!": 'the word "fine"',
        "!shortphrase!": "vertical line on the upper part of the staff",
        "!mediumphrase!": "same, but extending down to the centre line",
        "!longphrase!": "same, but extending 3/4 of the way down",
      };

      // ===================================

      const completionItems: vscode.CompletionItem[] = Object.entries(
        abcDecorations,
      ).map(([label, documentation]) => {
        const item = new vscode.CompletionItem(
          label,
          vscode.CompletionItemKind.Function,
        );
        item.range = new vscode.Range(position.translate(0, -1), position);
        item.documentation = new vscode.MarkdownString(documentation);
        item.insertText = new vscode.SnippetString(label);
        return item;
      });

      // ===================================

      // staccato
      const staccatoCompletion = new vscode.CompletionItem("!staccato!");
      staccatoCompletion.kind = vscode.CompletionItemKind.Function;
      staccatoCompletion.range = new vscode.Range(
        position.translate(0, -1),
        position,
      );
      staccatoCompletion.insertText = ".";
      staccatoCompletion.documentation = new vscode.MarkdownString(
        "Insert a staccato dot.",
      );
      completionItems.push(staccatoCompletion);

      return completionItems;
    },
  },
  "!",
);
