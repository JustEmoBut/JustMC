import { mount } from "svelte";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";
import App from "./App.svelte";

document.addEventListener("contextmenu", (e) => e.preventDefault());

// Links in rendered markdown would navigate the webview itself; hand them to the OS browser.
document.addEventListener("click", (e) => {
  const a = (e.target as Element).closest?.("a[href]");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href")!;
  if (/^https?:\/\//i.test(href)) openUrl(href).catch((err) => console.error(err));
});

export default mount(App, { target: document.getElementById("app")! });
