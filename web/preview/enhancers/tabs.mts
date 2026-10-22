export function renderTabs(
  block: HTMLElement,
  activeTabs: Map<string, string>,
  nextId: () => number,
): void {
  const panels = [...block.children].filter(
    (child): child is HTMLElement =>
      child instanceof HTMLElement && child.dataset.fleximarkKind === "tab",
  );
  if (!panels.length) throw new Error("tabs requires at least one tab");
  block.querySelector(":scope > [role='tablist']")?.remove();
  const tablist = document.createElement("div");
  tablist.setAttribute("role", "tablist");
  const containerId = block.dataset.fleximarkNodeId ?? "";
  const buttons = panels.map((panel, index) => {
    const button = document.createElement("button");
    const suffix = `${nextId()}-${index}`;
    button.type = "button";
    button.id = `fleximark-tab-${suffix}`;
    button.textContent = panel.dataset.tabLabel ?? `Tab ${index + 1}`;
    button.setAttribute("role", "tab");
    button.setAttribute("aria-controls", `fleximark-panel-${suffix}`);
    panel.id = `fleximark-panel-${suffix}`;
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", button.id);
    const activate = () => {
      for (let position = 0; position < panels.length; position += 1) {
        const active = position === index;
        panels[position].hidden = !active;
        buttons[position].setAttribute("aria-selected", String(active));
        buttons[position].tabIndex = active ? 0 : -1;
      }
      const panelId = panel.dataset.fleximarkNodeId;
      if (containerId && panelId) activeTabs.set(containerId, panelId);
    };
    button.addEventListener("click", activate);
    button.addEventListener("keydown", (event) => {
      const next =
        event.key === "ArrowRight"
          ? (index + 1) % panels.length
          : event.key === "ArrowLeft"
            ? (index + panels.length - 1) % panels.length
            : event.key === "Home"
              ? 0
              : event.key === "End"
                ? panels.length - 1
                : undefined;
      if (next === undefined) return;
      event.preventDefault();
      buttons[next].click();
      buttons[next].focus();
    });
    tablist.append(button);
    return button;
  });
  block.prepend(tablist);
  const active = activeTabs.get(containerId);
  buttons[
    Math.max(
      0,
      panels.findIndex((panel) => panel.dataset.fleximarkNodeId === active),
    )
  ].click();
}
