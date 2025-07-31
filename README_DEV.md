# FlexiMark

## How to

### Pack

```
npx @vscode/vsce package
```

### Publish

```
npx @vscode/vsce publish
```

### l10n

After replacing the extensions of all `*.mts` files under `src` with `.ts`, run the following command.

```
npx @vscode/l10n-dev export --outDir ./l10n ./src
```

### Tests

change `package.json` 

```json
  "activationEvents": [
    "onLanguage:markdown",
    "*"
  ],
```

then,

```
npm test
```

## Project Diagram
wip

```mermaid
flowchart TD
  ui:command["`
    ui:command
    (VSCode Command Parrete)
  `"]
  ui:textEditor["`
    ui:textEditor
    (VSCode Text Editor)
  `"]
  ui:webview["`
    ui:webview
    (VSCode iframe)
  `"]
  ui:browser["`
    ui:browser
    (External Web Browser)
  `"] 
  core:webview-front["`
    core:webview-front
    (media/vscodeWebviewScrollScripts.mts)
  `"]
  core:browser-front["`
    core:webview-front
    (media/webSocketScripts.mts)
  `"]
  
  User --> ui:command --> core:filesystem
  
  User --
      Active file change
      File content edit
      Cursor move
      Scroll
  --> ui:textEditor
  
  User -- 
      Scroll
  --> ui:webview --
      scroll
  --> core:webview-front --
      preview-scroll
  --> core:webview-server --> ui:textEditor

  
  User -- 
      Scroll
  --> ui:browser --
      scroll
  --> core:browser-front --
      preview-scroll
  --> core:browser-server --> ui:textEditor
```