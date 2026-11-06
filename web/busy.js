// A server that is busy sending pages answers "503 Service Unavailable" with a Retry-After of
// seconds. Every file the page fetches from its own origin goes through here: a 503 says on the
// page that the server is busy and when it is tried again, waits as long as the server asked
// (within reason), and tries once more; a second 503 or any other failure is the caller's as
// before. Modules and workers that do not load are told the same way by `busyFor`.
//
// A classic script as much as a module (`import "./busy.js"`): boot.js uses it before any module
// has loaded, the page's modules and the import worker after. It is `globalThis.dbota_busy`.
(function () {
  "use strict";
  // Seconds waited when the server says no Retry-After, or one that is no number of seconds or
  // date, and the least and most taken from it.
  var DEFAULT_S = 5;
  var MIN_S = 1;
  var MAX_S = 60;

  // The seconds a busy answer asks to wait: its Retry-After (seconds, or an HTTP date), within
  // MIN_S to MAX_S; DEFAULT_S without a readable one.
  function seconds(response) {
    var header = (response && response.headers.get("retry-after")) || "";
    header = header.trim();
    var wait;
    if (/^\d{1,9}$/.test(header)) {
      wait = Number(header);
    } else {
      var at = /\d{4}/.test(header) ? Date.parse(header) : NaN;
      if (isNaN(at)) return DEFAULT_S;
      wait = Math.ceil((at - Date.now()) / 1000);
    }
    return Math.min(MAX_S, Math.max(MIN_S, wait));
  }

  // What the page says while it waits.
  function line(wait) {
    return "The server is busy: trying again in " + wait + " s...";
  }

  // Resolves after `wait` seconds, or rejects as `signal` aborts.
  function sleep(wait, signal) {
    return new Promise(function (resolve, reject) {
      if (signal && signal.aborted) return reject(signal.reason);
      var timer = setTimeout(resolve, wait * 1000);
      if (signal) {
        signal.addEventListener("abort", function () {
          clearTimeout(timer);
          reject(signal.reason);
        });
      }
    });
  }

  // `fetch`, tried once more after a busy answer: `onBusy(seconds)` is told before the wait.
  // The second answer is returned whatever it is.
  function busyFetch(url, init, onBusy) {
    init = init || {};
    return fetch(url, init).then(function (response) {
      if (response.status !== 503) return response;
      var wait = seconds(response);
      if (onBusy) onBusy(wait);
      if (response.body) response.body.cancel().catch(function () {});
      return sleep(wait, init.signal).then(function () {
        return fetch(url, init);
      });
    });
  }

  // A wasm module's response, for a wasm-bindgen `init`: fetched as `busyFetch` does, and an
  // error naming the file unless it arrived.
  function busyWasm(url, onBusy) {
    return busyFetch(url, {}, onBusy).then(function (response) {
      if (!response.ok) throw new Error(url + " answered " + response.status);
      return response;
    });
  }

  // After a script, module or worker of `urls` did not load: the seconds to wait when one of
  // them was turned away busy (a 503, as the browser's record of the load says, or as asking for
  // the file again answers), else null.
  function busyFor(urls) {
    var resolved = urls.map(function (url) {
      return new URL(url, location.href).href;
    });
    var seen = resolved.some(function (url) {
      var entries = performance.getEntriesByName ? performance.getEntriesByName(url) : [];
      return entries.some(function (e) {
        return e.responseStatus === 503;
      });
    });
    var asked = resolved.map(function (url) {
      return fetch(url, { cache: "no-store" }).then(
        function (response) {
          if (response.body) response.body.cancel().catch(function () {});
          return response.status === 503 ? seconds(response) : null;
        },
        function () {
          return null;
        }
      );
    });
    return Promise.all(asked).then(function (waits) {
      var busy = waits.filter(function (w) {
        return w !== null;
      });
      if (busy.length) return Math.max.apply(null, busy);
      return seen ? DEFAULT_S : null;
    });
  }

  // `import(url)`, tried once more if it failed while the server was busy: a module that failed
  // to load stays failed under its address, so the second try asks for it under another.
  function busyImport(url, onBusy) {
    return import(url).catch(function (error) {
      return busyFor([url]).then(function (wait) {
        if (wait === null) throw error;
        if (onBusy) onBusy(wait);
        return sleep(wait).then(function () {
          return import(url + (url.indexOf("?") < 0 ? "?" : "&") + "busy=1");
        });
      });
    });
  }

  globalThis.dbota_busy = {
    DEFAULT_S: DEFAULT_S,
    MIN_S: MIN_S,
    MAX_S: MAX_S,
    seconds: seconds,
    line: line,
    sleep: sleep,
    fetch: busyFetch,
    wasm: busyWasm,
    busyFor: busyFor,
    import: busyImport,
  };
})();
