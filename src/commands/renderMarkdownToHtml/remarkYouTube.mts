import { Html, Paragraph } from "mdast";
import { Plugin } from "unified";
import { Node, Parent } from "unist";
import { visit } from "unist-util-visit";

function extractYouTubeVideoId(url: string): string | null {
  try {
    const parsedUrl = new URL(url);
    const hostname = parsedUrl.hostname;
    const pathname = parsedUrl.pathname;

    // https://www.youtube.com/watch?v=VIDEO_ID
    if (
      hostname.includes("youtube.com") ||
      hostname.includes("youtube-nocookie.com")
    ) {
      // 1. watch?v=VIDEO_ID
      if (pathname === "/watch") {
        return parsedUrl.searchParams.get("v");
      }

      // 2. /embed/VIDEO_ID
      const embedMatch = pathname.match(/^\/embed\/([^/?#]+)/);
      if (embedMatch) {
        return embedMatch[1];
      }

      // 3. /shorts/VIDEO_ID
      const shortsMatch = pathname.match(/^\/shorts\/([^/?#]+)/);
      if (shortsMatch) {
        return shortsMatch[1];
      }
    }

    // https://youtu.be/VIDEO_ID
    if (hostname === "youtu.be") {
      const shortMatch = pathname.match(/^\/([^/?#]+)/);
      if (shortMatch) {
        return shortMatch[1];
      }
    }

    return null;
  } catch {
    return null;
  }
}

function getIframeYouTubeEmbed(videoId: string): string {
  return `
    <div style="position: relative; width: 100%; padding-bottom: 56.25%;">
      <iframe 
        src="https://www.youtube.com/embed/${videoId}" 
        style="position: absolute; top: 0; left: 0; width: 100%; height: 100%;" 
        frameborder="0" 
        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" 
        allowfullscreen>
      </iframe>
    </div>
  `.trim();
}

function getLazyYouTubeEmbed(videoId: string): string {
  const thumbnailUrl = `https://img.youtube.com/vi/${videoId}/hqdefault.jpg`;
  return `
    <div class="youtube-placeholder" data-video-id="${videoId}" style="position: relative; width: 100%; padding-bottom: 56.25%; cursor: pointer;">
      <img src="${thumbnailUrl}" style="position: absolute; top: 0; left: 0; width: 100%; height: 100%; object-fit: cover;">
      <div style="position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); width: 68px; height: 48px;">
        <svg viewBox="0 0 68 48" width="100%" height="100%">
          <path d="M66.52 7.01a8 8 0 0 0-5.63-5.66C56.23 0.4 34 0.4 34 0.4s-22.23 0-26.89.95A8 8 0 0 0 1.48 7.01 83.16 83.16 0 0 0 0 24a83.16 83.16 0 0 0 1.48 16.99 8 8 0 0 0 5.63 5.66c4.66.95 26.89.95 26.89.95s22.23 0 26.89-.95a8 8 0 0 0 5.63-5.66A83.16 83.16 0 0 0 68 24a83.16 83.16 0 0 0-1.48-16.99z" fill="#f00"/>
          <path d="M45 24 27 14v20z" fill="#fff"/>
        </svg>
      </div>
    </div>
  `.trim();
}

interface RemarkYouTubeOptions {
  mode?: "lazy" | "iframe";
}

const remarkYouTube: Plugin<[RemarkYouTubeOptions?]> = (options = {}) => {
  const { mode = "lazy" } = options;

  return (tree: Node) => {
    visit(
      tree,
      "paragraph",
      (node: Paragraph, index: number | null, parent: Parent | null) => {
        if (!parent || !node.children || node.children.length !== 1) return;

        // if (parent.type !== "root") return;

        const child = node.children[0];
        if (child.type !== "link") return;

        const videoId = extractYouTubeVideoId(child.url);
        if (!videoId) return;

        // exchange Paragraph to Html
        const htmlContent =
          mode === "lazy"
            ? getLazyYouTubeEmbed(videoId)
            : getIframeYouTubeEmbed(videoId);
        const htmlNode: Html = {
          type: "html",
          value: htmlContent,
          data: {
            hName: "div",
            hProperties: {
              className: "embed-content youtube-embed",
            },
          },
          position: node.position,
        };

        // replace the paragraph with the HTML node
        if (index !== null && parent.children) {
          parent.children[index] = htmlNode;
        } else {
          // If index is null, append the HTML node to the parent
          parent.children.push(htmlNode);
        }
      },
    );
  };
};

export default remarkYouTube;
