/* ==========================================================================
   Theme bootstrap — runs in <head>, before the first paint.

   Dark is the app; light is the documented second reading. Only one value is
   ever stored, because "dark" is what you get when nothing is stored. Reading it
   here rather than at the end of <body> is the whole job of this file: a light
   window that paints one dark frame before main.js loads is a bug you only see
   for 60ms and then never stop seeing.

   ?theme=light on any page does the same thing without touching storage, which
   is how the light theme gets reviewed page by page.
   ========================================================================== */

(function () {
  "use strict";

  try {
    var fromQuery = /[?&]theme=(light|dark)\b/.exec(window.location.search);
    var theme = fromQuery ? fromQuery[1] : window.localStorage.getItem("takofy-theme");
    if (theme === "light") document.documentElement.setAttribute("data-theme", "light");
    else if (theme === "dark") document.documentElement.removeAttribute("data-theme");
  } catch (err) {
    /* No storage (private mode, file://): the app stays dark, which is correct. */
  }
})();
