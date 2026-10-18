(() => {
  let stored;
  try { stored = localStorage.getItem("notegate.theme"); } catch { /* Storage is optional. */ }
  const root = document.documentElement;
  root.dataset.theme = stored === "light" || stored === "dark"
    ? stored
    : matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  document.addEventListener("DOMContentLoaded", () => {
    const toggle = document.getElementById("auth-theme");
    const label = () => { toggle.textContent = `Use ${root.dataset.theme === "dark" ? "light" : "dark"} theme`; };
    toggle.hidden = false;
    label();
    toggle.addEventListener("click", () => {
      root.dataset.theme = root.dataset.theme === "dark" ? "light" : "dark";
      try { localStorage.setItem("notegate.theme", root.dataset.theme); } catch { /* Storage is optional. */ }
      label();
    });
    for (const time of document.querySelectorAll("time[data-auth-time]")) {
      const date = new Date(time.dateTime);
      if (!Number.isNaN(date.getTime())) {
        time.textContent = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
      }
    }
  });
})();
