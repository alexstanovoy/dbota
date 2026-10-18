// With ?report, every console line (the client logs its memory every five seconds) goes to the
// page's own server as a request to /report, for its log: a debugging aid for local runs.
const report = new URLSearchParams(location.search).has("report");
const send = (m) => {
  if (report) fetch("/report?" + encodeURIComponent(String(m).slice(0, 600))).catch(() => {});
};
for (const k of ["log", "info", "warn", "error"]) {
  const orig = console[k].bind(console);
  console[k] = (...a) => {
    orig(...a);
    send(k + ": " + a.join(" "));
  };
}
addEventListener("error", (e) => send("onerror: " + e.message));
send("start: webgpu " + !!navigator.gpu);
