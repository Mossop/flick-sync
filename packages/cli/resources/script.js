function updateSidebar() {
  let uri = new URL(document.documentURI);
  let currentPath = uri.pathname;

  for (let link of document.querySelectorAll(".sidebar-item")) {
    let isSelected;

    if (link.getAttribute("href") == "/") {
      isSelected = currentPath == "/";
    } else {
      isSelected = currentPath.startsWith(link.getAttribute("href"));
    }

    if (isSelected) {
      link.classList.add("selected");
    } else {
      link.classList.remove("selected");
    }
  }
}

document.addEventListener("DOMContentLoaded", updateSidebar);

htmx.on("htmx:historyRestore", updateSidebar);

htmx.on("htmx:pushedIntoHistory", updateSidebar);

htmx.on("htmx:replacedIntoHistory", updateSidebar);
