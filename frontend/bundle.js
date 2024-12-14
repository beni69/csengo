// styles
require("@picocss/pico");
require("./style.css");

const htmx = require("htmx.org").default;
globalThis.htmx = htmx; // xd

document.addEventListener("htmx:sendError", e => {
    alert("Failed to connect to server");
    console.error(e);
});
document.addEventListener("htmx:responseError", e => {
    alert(`${e.detail.xhr.statusText}: ${e.detail.xhr.response}`);
    console.error(e);
});

htmx.on("form#task", "htmx:configRequest", e => {
    if (e.detail.elt.id !== "task") return;
    e.detail.parameters["priority"] ??= 0;
});

htmx.on("#fileupload", "htmx:xhr:progress", function(e) {
    htmx.find("#fileupload #progress").setAttribute("value", e.detail.loaded / e.detail.total * 100)
});

const sleep = async ms => new Promise(r => setTimeout(r, ms));
document.addEventListener("DOMContentLoaded", async () => {
    while (true) {
        console.debug("realtime sub");
        const res = await fetch("/htmx/status/realtime");

        if (res.ok) {
            const resHTML = await res.text();
            console.debug("realtime recv", resHTML);
            document.getElementById("status").outerHTML = resHTML;
            htmx.process(document.getElementById("status"));
        } else {
            console.error("realtime error", res);
        }
        await sleep(250);
    }
});
