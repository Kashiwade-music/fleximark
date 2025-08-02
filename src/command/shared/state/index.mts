import * as fServer from "../../server/index.mjs";

class State {
  public webviewServer: fServer.WebviewServer;
  public browserServer: fServer.BrowserServer;

  constructor() {
    this.webviewServer = new fServer.WebviewServer();
    this.browserServer = new fServer.BrowserServer();
  }

  private getAsArray(): fServer.BaseServer[] {
    return [this.webviewServer, this.browserServer];
  }

  private getFirstDefined<K extends keyof fServer.BaseServer>(
    key: K,
  ): fServer.BaseServer[K] | undefined {
    return this.getAsArray().find((item) => item[key] !== undefined)?.[key];
  }

  public getHtml() {
    return this.getFirstDefined("html");
  }

  public getHast() {
    return this.getFirstDefined("hast");
  }

  public getMdast() {
    return this.getFirstDefined("mdast");
  }

  public getPlainText() {
    return this.getFirstDefined("plainText");
  }

  public getEditorPanel() {
    return this.getFirstDefined("editorPanel");
  }
}

export default State;
