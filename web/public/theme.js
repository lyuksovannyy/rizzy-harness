// Applies the saved theme and accent before the first paint, from the
// mirrors src/lib/appearance.ts keeps. A file, not an inline script: the
// page's CSP allows scripts from this origin only.
(function () {
  try {
    var d = document.documentElement;
    var t = localStorage.getItem("harness.theme");
    var a = localStorage.getItem("harness.accent");
    if (t === "light" || t === "dark") d.setAttribute("data-theme", t);
    if (a && /^(purple|pink|red|orange|yellow|green|graphite)$/.test(a)) d.setAttribute("data-accent", a);
  } catch (e) {}
})();
