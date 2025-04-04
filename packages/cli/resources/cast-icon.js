import { LitElement, html, nothing } from "lit";
import "@shoelace/components/icon-button/icon-button.js";

export class CastIcon extends LitElement {
  static properties = {
    isCasting: { state: true },
    url: { state: true },
    title: { state: true },
  };

  constructor() {
    super();
    this.isCasting = false;
  }

  initCast = () => {
    this.isCastAvailable = true;

    let castContext = cast.framework.CastContext.getInstance();
    if (castContext.getCurrentSession()) {
      this.updateCastSession(castContext.getCurrentSession());
    }
    castContext.addEventListener(
      cast.framework.CastContextEventType.SESSION_STATE_CHANGED,
      this.castContextEventListener
    );
  };

  castContextEventListener = (event) => {
    this.updateCastSession(event.session);
  };

  updateMediaSession(mediaInfo) {
    if (mediaInfo) {
      this.isCasting = true;
      this.url = new URL(mediaInfo.contentId, document.documentURI);
      this.title = mediaInfo.metadata.title;
    } else {
      this.isCasting = false;
    }
  }

  updateCastSession(session) {
    if (
      session &&
      [
        cast.framework.SessionState.NO_SESSION,
        cast.framework.SessionState.SESSION_START_FAILED,
        cast.framework.SessionState.SESSION_ENDING,
        cast.framework.SessionState.SESSION_ENDED,
      ].includes(session.getSessionState())
    ) {
      session = null;
    }

    if (this.castSession == session) {
      return;
    }

    this.castSession = session;

    if (session) {
      session.addEventListener(
        cast.framework.SessionEventType.MEDIA_SESSION,
        (event) => {
          this.updateMediaSession(event.mediaSession?.media);
        }
      );

      this.updateMediaSession(session.getMediaSession()?.media);
    } else {
      this.updateMediaSession(null);
    }
  }

  connectedCallback() {
    super.connectedCallback();

    if (window.castAvailable) {
      this.initCast();
    } else {
      document.addEventListener("cast-available", this.initCast, {
        once: true,
      });
    }
  }

  disconnectedCallback() {
    document.removeEventListener("cast-available", this.initCast);

    super.disconnectedCallback();
  }

  createRenderRoot() {
    return this;
  }

  render() {
    if (!this.isCasting) {
      return nothing;
    }

    return html`
      <a class="sidebar-item" href="${this.url}">
        <sl-icon name="broadcast"></sl-icon> ${this.title}
      </a>
    `;
  }
}

customElements.define("cast-icon", CastIcon);
