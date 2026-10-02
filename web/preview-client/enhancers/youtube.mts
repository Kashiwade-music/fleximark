export function renderYouTube(
  block: HTMLElement,
  current: () => boolean,
): void {
  const source = block.dataset.source;
  if (!source) throw new Error("missing YouTube source");
  const url = new URL(source);
  if (url.protocol !== "https:") throw new Error("invalid YouTube URL");
  const id =
    url.hostname === "youtu.be" && /^\/[A-Za-z0-9_-]{11}$/.test(url.pathname)
      ? url.pathname.slice(1)
      : (url.hostname === "youtube.com" ||
            url.hostname === "www.youtube.com") &&
          url.pathname === "/watch"
        ? url.searchParams.get("v")
        : undefined;
  if (!id || !/^[A-Za-z0-9_-]{11}$/.test(id))
    throw new Error("invalid YouTube video id");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "youtube-placeholder";
  button.setAttribute("aria-label", "Play YouTube video");
  const thumbnail = document.createElement("img");
  thumbnail.src = `https://i.ytimg.com/vi/${id}/hqdefault.jpg`;
  thumbnail.alt = "";
  thumbnail.loading = "lazy";
  button.append(thumbnail);
  button.dataset.fleximarkYoutubeConsent = "true";
  button.addEventListener("click", () => {
    if (!current() || !button.isConnected) return;
    const frame = document.createElement("iframe");
    frame.src = `https://www.youtube-nocookie.com/embed/${id}`;
    frame.className = "fleximark-youtube-player";
    frame.title = "YouTube video player";
    frame.loading = "lazy";
    frame.allowFullscreen = true;
    frame.setAttribute(
      "allow",
      "accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share",
    );
    frame.setAttribute("referrerpolicy", "strict-origin-when-cross-origin");
    frame.setAttribute(
      "sandbox",
      "allow-scripts allow-same-origin allow-presentation",
    );
    button.replaceWith(frame);
  });
  block.replaceChildren(button);
}
