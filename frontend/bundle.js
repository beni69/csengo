// styles
require("@picocss/pico");
require("./style.css");

const htmx = require("htmx.org").default;
globalThis.htmx = htmx; // xd

// Toast notification system
function showToast(message, type = "info", duration = 5000) {
    const container = document.getElementById("toast-container");
    if (!container) return;

    const toast = document.createElement("div");
    toast.className = `toast ${type}`;
    toast.textContent = message;
    container.appendChild(toast);

    const dismiss = () => {
        toast.classList.add("fade-out");
        toast.addEventListener("animationend", () => toast.remove());
    };

    toast.addEventListener("click", dismiss);
    setTimeout(dismiss, duration);
}

// Error handlers - replace alerts with toasts
document.addEventListener("htmx:sendError", e => {
    showToast("A szerverhez való csatlakozás sikertelen", "error");
    console.error(e);
});
document.addEventListener("htmx:responseError", e => {
    showToast(`${e.detail.xhr.statusText}: ${e.detail.xhr.response}`, "error");
    console.error(e);
});

// Success notifications
document.addEventListener("htmx:afterRequest", e => {
    // Only show toast for successful requests
    if (!e.detail.successful) return;

    const trigger = e.detail.requestConfig?.elt;
    if (!trigger) return;

    // Task form submission
    if (trigger.id === "task") {
        showToast("Csengetés hozzáadva", "success");
    }
    // File upload
    else if (trigger.id === "fileupload") {
        showToast("File feltöltve", "success");
        const progress = document.getElementById("fileupload-progress");
        if (progress) progress.setAttribute("value", 0);
    }
    // Delete operations
    else if (trigger.classList?.contains("delete")) {
        showToast("Törölve", "success");
    }
    // Stop button
    else if (trigger.id === "btn-stop") {
        showToast("Leállítva", "success");
    }
});

htmx.on("form#task", "htmx:configRequest", e => {
    if (e.detail.elt.id !== "task") return;
    e.detail.parameters["priority"] ??= 0;
});

htmx.on("#fileupload", "htmx:xhr:progress", function(e) {
    htmx.find("#fileupload-progress").setAttribute("value", e.detail.loaded / e.detail.total * 100)
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
