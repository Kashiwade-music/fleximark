# FlexiMark for VS Code

FlexiMark provides an embedded Markdown preview and a shared browser workspace for notes, templates, themes, admonitions and HTML export. Preview rendering includes Mermaid, ABC notation and KaTeX.

Use **FlexiMark: Preview Markdown** from a Markdown editor. **Open Workspace in Browser** opens the complete workspace interface. **Reveal Selection in Browser** navigates an external preview to the current source selection.

Workspace files use `.fleximark/config.toml`. Workspace writes and local asset access require trust. To convert an old FlexiMark workspace, explicitly run **Migrate legacy FlexiMark workspace**. Startup does not import old settings or files.

Each platform-specific VSIX includes the same verified runtime archive released for the CLI and Zed. Its `runtime/manifest.json` binds both binaries and browser resources by SHA-256. A custom `fleximark.daemonPath` selects a separately managed runtime.
