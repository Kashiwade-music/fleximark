# FlexiMark

## Pack

```
npx @vscode/vsce package
```

## l10n

```
npx @vscode/l10n-dev export --outDir ./l10n ./src
```

## Tests

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