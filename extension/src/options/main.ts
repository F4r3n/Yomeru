import { mount } from "svelte";
import Options from "./Options.svelte";

mount(Options, { target: document.getElementById("app")! });
document.getElementById("loading")?.remove();
