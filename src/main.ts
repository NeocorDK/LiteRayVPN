import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// The window is a native app, not a page: block the browser context menu and reload keys.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea")) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r")) e.preventDefault();
});

export default mount(App, { target: document.getElementById("app")! });
