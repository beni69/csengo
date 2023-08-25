const htmx = require("htmx.org");
globalThis.htmx = htmx; // xd

require("@picocss/pico");
require("./style.css");

// const dayjs = require("dayjs");
// require("dayjs/locale/hu");
// const relTime = require("dayjs/plugin/relativeTime");
//
// dayjs.locale("hu");
// dayjs.extend(relTime);
//
// globalThis.durFmt = (_one, _two) => {
//     const one = dayjs(_one);
//     const two = dayjs(_two);
//     if (one > two) {
//         return one.from(two);
//     } else {
//         return two.from(one);
//     }
// }

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
(async () => {
    while (true) {
        console.debug("sub realtime");
        const res = await fetch("/htmx/status/realtime").then(r => r.text());
        console.debug("recv realtime", res);
        document.getElementById("status").outerHTML = res;
        await sleep(250);
    }
})()
