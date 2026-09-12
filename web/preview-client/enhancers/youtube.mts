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
  button.textContent = "Load YouTube video";
  button.dataset.fleximarkYoutubeConsent = "true";
  button.addEventListener("click", () => {
    if (!current() || !button.isConnected) return;
    const frame = document.createElement("iframe");
    frame.src = `https://www.youtube-nocookie.com/embed/${id}`;
    frame.title = "YouTube video";
    frame.loading = "lazy";
    frame.allowFullscreen = true;
    frame.setAttribute(
      "sandbox",
      "allow-scripts allow-same-origin allow-presentation",
    );
    button.replaceWith(frame);
  });
  block.replaceChildren(button);
}
