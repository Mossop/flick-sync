function updateLinks() {
  let uri = new URL(document.documentURI);
  let currentPath = uri.pathname;

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

function openSidebar() {
  document.body.classList.add("sidebar-open");
}

function closeSidebar() {
  document.body.classList.remove("sidebar-open");
}

function contentChanged() {
  updateLinks();
  closeSidebar();
}

document.addEventListener("DOMContentLoaded", contentChanged);

htmx.on("htmx:historyRestore", contentChanged);

htmx.on("htmx:pushedIntoHistory", contentChanged);

htmx.on("htmx:replacedIntoHistory", contentChanged);

htmx.on("htmx:afterSwap", contentChanged);

window.__onGCastApiAvailable = function (isAvailable) {
  if (isAvailable) {
    cast.framework.CastContext.getInstance().setOptions({
      receiverApplicationId: chrome.cast.media.DEFAULT_MEDIA_RECEIVER_APP_ID,
    });

    castAvailable = true;

    document.dispatchEvent(new CustomEvent("cast-available"));
  }
};

var castAvailable = false;
