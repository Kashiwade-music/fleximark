document.addEventListener("click", (e: MouseEvent) => {
  const target = e.target;
  if (!(target instanceof HTMLElement)) return;

  const placeholder = target.closest(
    ".youtube-placeholder"
  ) as HTMLElement | null;
  if (!placeholder) return;

  if (placeholder.querySelector("iframe")) return; // 二重追加を防止

  const videoId = placeholder.dataset.videoId;
  if (!videoId) return;

  const iframe = document.createElement("iframe");
  iframe.src = `https://www.youtube.com/embed/${videoId}?autoplay=1`;
  iframe.style.position = "absolute";
  iframe.style.top = "0";
  iframe.style.left = "0";
  iframe.style.width = "100%";
  iframe.style.height = "100%";

  iframe.setAttribute("frameborder", "0");
  iframe.setAttribute(
    "allow",
    "accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
  );
  iframe.setAttribute("allowfullscreen", "true");

  placeholder.innerHTML = "";
  placeholder.appendChild(iframe);
});
