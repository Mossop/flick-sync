function updateLinks(event) {
  let uri = new URL(document.documentURI);
  let currentPath = uri.pathname;
  console.log(event.type, currentPath);

  for (let link of document.querySelectorAll(".link-item")) {
    let href = link.getAttribute("href");

    let isSelected = currentPath == href;

    if (!isSelected && link.classList.contains("link-prefix")) {
      if (!href.endsWith("/")) {
        href += "/";
      }

      isSelected = currentPath.startsWith(href);
    }

    if (isSelected) {
      link.classList.add("selected");
    } else {
      link.classList.remove("selected");
    }
  }
}

document.addEventListener("DOMContentLoaded", updateLinks);

htmx.on("htmx:historyRestore", updateLinks);

htmx.on("htmx:pushedIntoHistory", updateLinks);

htmx.on("htmx:replacedIntoHistory", updateLinks);

htmx.on("htmx:afterSwap", updateLinks);
