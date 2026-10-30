// The page's first script, a classic one so that it runs where the page's modules cannot (a browser
// too old for them, a module that did not load, a syntax error in one): the page is never blank
// and never spins for ever. It shows "Loading" at once and the failure screen when the page's code
// has not started within WATCHDOG_MS, saying which stage it was. Later stages (storage, the
// graphics, the download of the game) have their own limits in main.js and end here too:
// `dbota_boot.fail` is the page's one failure screen.
(function () {
  "use strict";
  var WATCHDOG_MS = 30000;
  var $ = function (id) {
    return document.getElementById(id);
  };
  var ended = false;
  var started = false;
  var early = [];

  // Shows the failure screen once: what failed, what the player can do, details for a report.
  function fail(title, text, detail) {
    console.error("dbota: " + title + ". " + text + (detail ? "\n" + detail : ""));
    if (ended) return;
    ended = true;
    ["grant", "ready", "progress", "stale"].forEach(function (id) {
      $(id).hidden = true;
    });
    $("booting").hidden = true;
    $("starting").hidden = true;
    $("failTitle").textContent = title;
    $("failText").textContent = text;
    $("failMore").hidden = !detail;
    $("failDetail").textContent = detail || "";
    $("failed").hidden = false;
    $("gate").hidden = false;
  }

  var UPDATE = "Update the browser (a current Chrome, Edge or Firefox works) and reload this page.";
  var network = "Check the connection and reload this page; if it keeps happening, try another browser.";

  if (typeof WebAssembly !== "object" || !("noModule" in HTMLScriptElement.prototype)) {
    fail("This browser is too old for dbota", "dbota needs WebAssembly and JavaScript modules. " + UPDATE);
    return;
  }

  addEventListener(
    "error",
    function (e) {
      if (started) return;
      var t = e.target;
      if (t && t.tagName === "SCRIPT") {
        fail("A part of the page did not load", "The file " + (t.src || "?").split("/").pop() + " could not be fetched. " + network);
      } else if (e.message) {
        early.push(e.message + (e.filename ? " (" + e.filename.split("/").pop() + ":" + e.lineno + ")" : ""));
        // A script that throws while the page's code loads: an old browser's syntax error is one.
        fail("The page's code could not run in this browser", UPDATE, early.join("\n"));
      }
    },
    true
  );

  var timer = setTimeout(function () {
    if (started) return;
    fail(
      "The page's code did not start",
      "Its files were still loading after " + WATCHDOG_MS / 1000 + " seconds. " + network,
      early.join("\n")
    );
  }, WATCHDOG_MS);

  globalThis.dbota_boot = {
    fail: fail,
    // The page's code is running: its own stages take over from here.
    started: function () {
      started = true;
      clearTimeout(timer);
    },
  };
})();
