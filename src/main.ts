import { mount } from "svelte";
import "./styles.css";
import App from "./App.svelte";

document.addEventListener("contextmenu", (e) => e.preventDefault());

export default mount(App, { target: document.getElementById("app")! });
