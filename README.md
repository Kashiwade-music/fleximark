<div align="center">
  <img src="assets/logo.webp" alt="FlexiMark Logo" height="150"/>
  <h1>FlexiMark</h1>
  <p><strong>Next-Level Markdown Preview and Note-Taking for VSCode</strong></p>

  <p>
    <a href="https://marketplace.visualstudio.com/items?itemName=kashiwade.fleximark">
      <img src="https://img.shields.io/visual-studio-marketplace/v/kashiwade.fleximark?color=brightgreen&label=VS%20Code%20Marketplace&style=for-the-badge" alt="Marketplace Version" />
    </a>
  </p>
</div>

![top_demo](assets/demo_main.avif)

<h2 align="center">Getting Started</h2>

<p align="center">
  <a href="https://fleximark.kashiwade.works/docs/documents/tutorial"><strong>English</strong></a> | <a href="https://fleximark.kashiwade.works/ja/docs/documents/tutorial"><strong>日本語</strong></a>
</p>

## 🚀 Overview

FlexiMark transforms **Markdown in VSCode** into a fully-customizable, interactive workspace.  
Write, preview, organize, and present your notes — all without leaving your editor.

- ⚡ **Instant Live Preview** in both VSCode and your web browser  
- 🎨 **Customizable Layouts & Styles** using full JavaScript/CSS power  
- 🧩 **Extended Markdown Syntax**: tabs, admonitions, diagrams, sheet music, and more  
- 📂 **Smart File Management** for effortless note organization  

Whether you’re a **developer**, **writer**, **researcher**, or **student**, FlexiMark makes your Markdown workflow faster, clearer, and far more flexible.

## ✨ Key Features

### 🔁 **Live Preview**

- **VSCode Live Preview**: – Instant, side-by-side preview as you type.
  ![VSCode Live Preview](assets/demo_vscode_preview.avif)
- **Web Browser Preview**: Render Markdown in a browser, with support for JavaScript execution, custom scripts, and styles.  
  ![Web Browser Preview](assets/demo_browser_preview.avif)

### 🧩 **Flexible File Generation**

- Auto-generate **customizable directory structures** based on your category tree.
- Configure **prefixes and suffixes** for filenames.
- Choose from **multiple, customizable templates** when creating new notes.
- Stay organized with minimal effort.

![Flexible File Generation](assets/demo_create_note.avif)

### 🧪 **Enhanced Markdown Features**

Based on GitHub Flavored Markdown (GFM) with advanced extensions:

- **Admonitions**: Notes, tips, warnings, dangers.
  ![Admonitions](assets/demo_admonitions.avif)
- **Tabs**: Organize content into neat sections.
  ![Tabs](assets/demo_tab.webp)
- **Collapsible Sections**: An alternative to `<details>`.
  ![Collapsible Sections](assets/demo_details.webp)
- **Embedded YouTube Videos**:
  ![Youtube iframe](assets/demo_youtube_iframe.webp)
- **Flexible Code Block**: Titles, line numbers, highlighting.
  ![Code Block](assets/demo_code_block.webp)
- **Mermaid Diagrams**: Flowcharts, sequences, and more.
  ![Mermaid Supports](assets/demo_mermaid.webp)
  Uses [this extension](https://marketplace.visualstudio.com/items?itemName=bpruitt-goddard.mermaid-markdown-syntax-highlighting) for syntax highlighting.
- **ABC Notation**: Render and play sheet music directly in Markdown.
  ![ABC Notation](assets/demo_abc.webp)
  - Realtime render and **play sheet music** in Markdown.
  - Supports **live preview**, **syntax highlighting**, and **useful snippets**.

### 🔧 **Other Useful Features**

- Export to Portable HTML files
- Extending Markdown Syntax
  ![Extending Markdown Syntax](assets/demo_extending_syntax.webp)
- Collect Admonitions/Alerts under specific categories
  ![Collect Admonitions](assets/demo_collect_admonitions.avif)

## 🧰 Ideal For

- Developers maintaining technical notes or documentation.
- Writers and researchers building structured content.
- Musicians using abc notation.
- Anyone wanting a smarter, more interactive Markdown workflow.

## 📦 Installation

Available via the [VSCode Marketplace](https://marketplace.visualstudio.com/items?itemName=Kashiwade.fleximark) or install manually:

```bash
code --install-extension kashiwade.fleximark
```

## 🚀 Getting Started

1. **Set up a new workspace** for your FlexiMark notes.

   > If you're migrating from the VSCode Note Taking Extension, use the [Migration Tool](https://github.com/Kashiwade-music/fleximark-migration-tool) to carry over your existing notes.

2. Open the **VSCode Command Palette** and run:
   `FlexiMark: Initialize Workspace as Note Taking Directory`

3. Customize your note categories in the generated configuration file.

4. Start writing! Use the Command Palette and select:
   `FlexiMark: Create New Note`

## 📚 Documentation

Detailed guides, configuration options, and templates are available in the [**FlexiMark Official Docs**](https://fleximark.kashiwade.works).

## 💬 Feedback & Contributions

FlexiMark is open source and welcomes contributions!
Found a bug or have a feature request? [Open an issue](#) or submit a pull request.

## 📄 License

MIT License. See [LICENSE](./LICENSE) for more details.

---

FlexiMark — _Take Markdown Notes. Preview Your Way._
