import { mount } from "nsh";
import { App } from "./src/App";
import { scanner } from "./src/runtime";

mount(() => <App scanner={scanner} />);
