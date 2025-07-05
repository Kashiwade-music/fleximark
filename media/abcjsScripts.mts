import abcjs, { EventCallbackReturn, NoteTimingEvent } from "abcjs";

class TimingCallbackState {
  id: number;
  visualObj: any[];
  lastEls: HTMLElement[][] = [];
  isRunning: boolean = false;
  stoppedByPause: boolean = false;
  stoppedByEnd: boolean = false;
  cursor: SVGLineElement;
  timingCallback: any;

  constructor(id: number, visualObj: any[]) {
    this.id = id;
    this.visualObj = visualObj;
    this.cursor = this.createCursor();
    this.timingCallback = new abcjs.TimingCallbacks(this.visualObj[0], {
      beatCallback: this.beatCallback.bind(this),
      eventCallback: this.eventCallback.bind(this),
    });
  }

  private createCursor(): SVGLineElement {
    const svg = document.querySelector(`#score${this.id} svg`) as SVGSVGElement;
    const cursor = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "line"
    );
    cursor.setAttribute("class", "abcjs-cursor");
    cursor.setAttributeNS(null, "x1", "0");
    cursor.setAttributeNS(null, "y1", "0");
    cursor.setAttributeNS(null, "x2", "0");
    cursor.setAttributeNS(null, "y2", "0");
    svg?.appendChild(cursor);
    return cursor;
  }

  private beatCallback(
    currentBeat: number,
    totalBeats: number,
    _lastMoment: any,
    position: { left: number; top: number; height: number },
    _debugInfo: any
  ): void {
    let x1: number, x2: number, y1: number, y2: number;
    if (currentBeat === totalBeats) {
      x1 = x2 = y1 = y2 = 0;
      this.stoppedByEnd = true;
      this.isRunning = false;
    } else {
      x1 = x2 = position.left - 2;
      y1 = position.top;
      y2 = position.top + position.height;
    }

    this.cursor.setAttribute("x1", x1.toString());
    this.cursor.setAttribute("x2", x2.toString());
    this.cursor.setAttribute("y1", y1.toString());
    this.cursor.setAttribute("y2", y2.toString());
  }

  private eventCallback(ev: NoteTimingEvent | null): EventCallbackReturn {
    if (!ev) {
      console.log("Event is null. Stopping.");
      this.isRunning = false;
      this.stoppedByPause = true;
      this.colorElements([]);
      this.timingCallback.reset();
    } else {
      console.log("Event is not null. Running.");
      if (ev.elements) {
        this.colorElements(ev.elements);
      }
      return "continue";
    }
  }

  private colorElements(els: HTMLElement[][]): void {
    this.lastEls.forEach((group) =>
      group.forEach((el) => el.classList.remove("color"))
    );
    els.forEach((group) => group.forEach((el) => el.classList.add("color")));
    this.lastEls = els;
  }
}

const timingCallbacksStateArray: TimingCallbackState[] = [];
let isRenderedABC = false;

window.addEventListener("load", () => {
  console.log("ページが完全に読み込まれました");
  renderABC();
});

function renderABC(): void {
  if (isRenderedABC) return;
  console.log("レンダリング中");

  const preElements = document.querySelectorAll(
    'pre[data-language="abc"]'
  ) as NodeListOf<HTMLPreElement>;

  preElements.forEach((preElement, idx) => {
    // ABC記譜テキストの抽出
    const codeElement = preElement.querySelector("code");
    if (!codeElement) return;

    const abcLines: string[] = [];
    codeElement.querySelectorAll("span[data-line]").forEach((lineSpan) => {
      abcLines.push(lineSpan.textContent || "");
    });
    const abcText = abcLines.join("\n");

    // 元のpreを破壊してdiv.scoreとdiv.audioを挿入
    const scoreDiv = document.createElement("div");
    scoreDiv.className = "score";
    scoreDiv.id = "score" + idx;
    scoreDiv.innerHTML = abcText;

    const audioDiv = document.createElement("div");
    audioDiv.className = "audio";
    audioDiv.id = "audio" + idx;

    // preを削除し、代わりにscoreDivとaudioDivを挿入
    const parent = preElement.parentElement;
    parent?.replaceChild(scoreDiv, preElement);
    parent?.insertBefore(audioDiv, scoreDiv.nextSibling);

    // ABCJSの描画
    const visualObj = abcjs.renderAbc(scoreDiv, abcText, {
      responsive: "resize",
    });
    const synthControl = new abcjs.synth.SynthController();

    synthControl.load("#audio" + idx, null, {
      displayRestart: true,
      displayPlay: true,
      displayProgress: true,
    });
    synthControl.setTune(visualObj[0], false);

    const timingState = new TimingCallbackState(idx, visualObj);
    timingCallbacksStateArray.push(timingState);

    const startStop = () => {
      const state = timingCallbacksStateArray[idx];
      if (state.stoppedByEnd) {
        state.stoppedByEnd = false;
        state.stoppedByPause = false;
        state.timingCallback.reset();
      }
      if (state.stoppedByPause) {
        state.stoppedByPause = false;
      }
      state.isRunning = !state.isRunning;
      if (state.isRunning) {
        state.timingCallback.start();
      } else {
        state.stoppedByPause = true;
        state.timingCallback.pause();
      }
    };

    const reset = () => {
      const state = timingCallbacksStateArray[idx];
      state.timingCallback.reset();
      state.isRunning = false;
    };

    const startButton = document.querySelector(
      `#audio${idx} > div > button.abcjs-midi-start.abcjs-btn`
    );
    startButton?.addEventListener("click", startStop);

    const resetButton = document.querySelector(
      `#audio${idx} > div > button.abcjs-midi-reset.abcjs-btn`
    );
    resetButton?.addEventListener("click", reset);
  });

  isRenderedABC = true;
}
