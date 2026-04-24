import { registerIconLibrary } from "@shoelace/utilities/icon-library.js";

registerIconLibrary("material", {
  resolver: (name) => {
    const match = name.match(/^(.*?)(_(round|sharp))?$/);
    return `/resources/material-icons/${match[1]}/${match[3] || "outline"}.svg`;
  },
  mutator: (svg) => svg.setAttribute("fill", "currentColor"),
});
