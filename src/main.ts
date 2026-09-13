import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";

// Expose the host OS to CSS. Only the panel's translucency needs it today: the
// webview UA is the cheapest signal, and a wrong guess only costs a slightly
// different background alpha. Windows and Linux are the only supported
// targets — anything else (there is no "else" in practice) falls to "other".
const ua = navigator.userAgent;
document.documentElement.dataset.os = /Windows/.test(ua)
  ? "windows"
  : /Linux|X11/.test(ua)
    ? "linux"
    : "other";

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
