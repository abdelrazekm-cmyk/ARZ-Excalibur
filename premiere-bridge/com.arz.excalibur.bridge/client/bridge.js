// Auto-spawns bin/ARZ-Excalibur.exe strictly when Premiere Pro opens,
// and shuts it down cleanly the moment Premiere Pro closes.
(function () {
    var BRIDGE_URL = "http://127.0.0.1:49872";
    var cep = window.__adobe_cep__;
    var hostEnv = cep ? JSON.parse(cep.getHostEnvironment()) : { appVersion: "unknown" };
    var childProc = null;

    function launchRustEngineIfNeeded() {
        try {
            if (typeof require !== "function") return;
            var path = require("path");
            var cp = require("child_process");
            var htmlFile = decodeURIComponent(window.location.pathname).replace(/^\/([A-Za-z]:)/, "$1");
            var binDir = path.resolve(path.dirname(htmlFile), "..", "bin");
            var exePath = path.join(binDir, "ARZ-Excalibur.exe");

            childProc = cp.spawn(exePath, ["--managed-by-premiere"], {
                cwd: binDir,
                windowsHide: true,
                stdio: "ignore"
            });
            childProc.on("error", function () { childProc = null; });
            childProc.on("exit", function () { childProc = null; });
        } catch (_e) {}
    }

    window.addEventListener("beforeunload", function () {
        try {
            fetch(BRIDGE_URL + "/shutdown", { method: "POST", keepalive: true });
        } catch (_e) {}
        if (childProc) {
            try { childProc.kill(); } catch (_e) {}
        }
    });

    function evalInPremiere(cmdObj) {
        return new Promise(function (resolve) {
            if (!cep) {
                resolve(JSON.stringify({ ok: false, error: "CEP unavailable" }));
                return;
            }
            var payload = JSON.stringify(cmdObj).replace(/\\/g, "\\\\").replace(/'/g, "\\'");
            cep.evalScript("$.arzExcalibur.dispatch('" + payload + "')", function (res) {
                resolve(res || "{}");
            });
        });
    }

    var failedPings = 0;
    async function pollLoop() {
        try {
            var resp = await fetch(BRIDGE_URL + "/poll?ver=" + encodeURIComponent(hostEnv.appVersion));
            if (resp.ok) {
                failedPings = 0;
                var commands = await resp.json();
                for (var i = 0; i < commands.length; i++) {
                    var result = await evalInPremiere(commands[i]);
                    await fetch(BRIDGE_URL + "/result", {
                        method: "POST",
                        body: result
                    });
                }
            }
        } catch (_err) {
            failedPings++;
            if (failedPings === 1 || failedPings % 25 === 0) {
                launchRustEngineIfNeeded();
            }
        }
        setTimeout(pollLoop, 120);
    }

    launchRustEngineIfNeeded();
    setTimeout(pollLoop, 300);
})();
