from __future__ import annotations

import base64
import re
from pathlib import Path

from _tools import ROOT, script_entrypoint


def vendor_katex_css() -> None:
    source = ROOT / "node_modules" / "katex" / "dist" / "katex.min.css"
    fonts = source.parent / "fonts"
    output = ROOT / "web" / "preview-client" / "katex.css"
    css = source.read_text(encoding="utf-8")

    def inline_woff2(match: re.Match[str]) -> str:
        filename = match.group(1)
        encoded = base64.b64encode((fonts / filename).read_bytes()).decode("ascii")
        return f'url("data:font/woff2;base64,{encoded}") format("woff2")'

    css = re.sub(
        r'url\(fonts/([^)]*\.woff2)\) format\("woff2"\),url\(fonts/[^)]*\.woff\) format\("woff"\),url\(fonts/[^)]*\.ttf\) format\("truetype"\)',
        inline_woff2,
        css,
    )
    if "url(fonts/" in css:
        raise RuntimeError("KaTeX CSS still contains external font URLs")
    output.write_text(
        "/* Generated from katex/dist/katex.min.css by scripts/vendor_katex_css.py. */\n"
        + css
        + "\n",
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    script_entrypoint(vendor_katex_css)
