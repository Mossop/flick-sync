import { LitElement, html, css, nothing, ref, keyed } from "lit";
import shoelaceStyles from "@shoelace/themes/dark.styles.js";
import "@shoelace/components/icon-button/icon-button.js";

function pad(val) {
  if (val > 9) {
    return val.toString();
  }

  return `0${val}`;
}

function formatTime(time) {
  time = Math.round(time);
  let seconds = time % 60;
  time = (time - seconds) / 60;
  let minutes = time % 60;
  let hours = (time - minutes) / 60;

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  } else {
    return `${pad(minutes)}:${pad(seconds)}`;
  }
}

export class VideoPlayer extends LitElement {
  static styles = [
    shoelaceStyles,
    css`
      :host {
        position: relative;

        &.casting .overlay {
          opacity: 1;
        }
      }

      video {
        width: 100%;
        height: 100%;
        object-fit: contain;
        object-position: center center;
      }

      .overlay {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: stretch;

        opacity: 0;
        transition: opacity var(--sl-transition-slow)
          cubic-bezier(0.4, 0, 0.2, 1) 0ms;

        &.visible {
          opacity: 1;
        }
      }

      .main {
        flex: 1;
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-evenly;
        font-size: 400%;
      }

      .controls {
        background: var(--sl-color-neutral-300);
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: var(--sl-spacing-x-small);

        --connected-color: var(--sl-color-primary-600);
        --disconnected-color: var(--sl-color-neutral-600);
      }

      sl-icon-button {
        font-size: 150%;
      }

      .time {
        width: 4.2em;

        &.start {
          text-align: end;
        }

        &.end {
          text-align: start;
        }
      }

      google-cast-launcher {
        height: 1.5em;

        &:hover {
          --connected-color: var(--sl-color-primary-700);
          --disconnected-color: var(--sl-color-neutral-700);
        }
      }

      .progress {
        flex: 1;
        background-color: black;
        height: 10px;
        margin-inline: var(--sl-spacing-x-small);
        position: relative;

        .buffer {
          background-color: var(--sl-color-neutral-100);
          position: absolute;
          top: 0;
          bottom: 0;
        }

        .played {
          background-color: var(--sl-color-neutral-900);
          position: absolute;
          top: 0;
          left: 0;
          bottom: 0;
        }

        .mask {
          position: absolute;
          inset: -5px;
          background-color: transparent;
          border-color: var(--sl-color-neutral-300);
          border-style: solid;
          border-width: 5px;
          border-radius: 10px;
        }
      }
    `,
  ];

  static properties = {
    playlist: { type: Array },
    mediaIndex: { state: true },
    currentTime: { type: Number },
    isPlaying: { state: true },
    isFullscreen: { state: true },
    isCastAvailable: { state: true },
    isCasting: { state: true },
    airDate: { type: String },
    image: { type: String },
    title: { type: String },
    show: { type: Object },
    season: { type: Object },
    episode: { type: Object },
  };

  constructor() {
    super();
    this.currentTime = 0;
    this.lastReportedTime = 0;
    this.mediaIndex = 0;
    this.isPlaying = false;
    this.previousTime = 0;
    this.onFullscreenChanged();
    this.isCastAvailable = window.castAvailable;
    this.isCasting = false;
    this.videoElement = null;
    this.castSession = null;

    if (window.castAvailable) {
      this.initCast();
    } else {
      document.addEventListener("cast-available", () => this.initCast(), {
        once: true,
      });
    }

    this.addEventListener("fullscreenchange", () => this.onFullscreenChanged());
  }

  initCast() {
    this.isCastAvailable = true;

    let castContext = cast.framework.CastContext.getInstance();
    if (castContext.getCurrentSession()) {
      this.updateCastSession(castContext.getCurrentSession());
    }
    castContext.addEventListener(
      cast.framework.CastContextEventType.SESSION_STATE_CHANGED,
      this.castContextEventListener
    );
  }

  castContextEventListener = (event) => {
    this.updateCastSession(event.session);
  };

  castControllerEventListener = (event) => {
    this.isPlaying = !this.castPlayer.isPaused;
    this.updateTime(this.castPlayer.currentTime + this.previousTime);
  };

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

    if (this.castSession) {
      this.castController.removeEventListener(
        cast.framework.RemotePlayerEventType.ANY_CHANGE,
        this.castControllerEventListener
      );
      this.castPlayer = null;
      this.castController = null;
    }

    if (session) {
      this.castPlayer = new cast.framework.RemotePlayer();
      this.castController = new cast.framework.RemotePlayerController(
        this.castPlayer
      );
      this.castController.addEventListener(
        cast.framework.RemotePlayerEventType.ANY_CHANGE,
        this.castControllerEventListener
      );

      this.isCasting = true;
      this.classList.add("casting");
    } else {
      this.castPlayer = null;
      this.castController = null;

      this.isCasting = false;
      this.classList.remove("casting");
    }

    this.castSession = session;

    this.mediaIndex = -1;
    this.seek(this.currentTime);
  }

  renderedVideo(element) {
    if (this.videoElement != element) {
      this.videoElement?.pause();
    }

    this.videoElement = element;

    if (this.videoElement) {
      this.videoElement.currentTime = this.currentTime - this.previousTime;
    }
  }

  connectedCallback() {
    super.connectedCallback();

    if (this.isCasting) {
      this.mediaIndex = -1;
      this.seek(this.currentTime);
    }
  }

  disconnectedCallback() {
    if (this.isCastAvailable) {
      cast.framework.CastContext.getInstance().removeEventListener(
        cast.framework.CastContextEventType.SESSION_STATE_CHANGED,
        this.castContextEventListener
      );

      if (this.castSession) {
        this.castController.removeEventListener(
          cast.framework.RemotePlayerEventType.ANY_CHANGE,
          this.castControllerEventListener
        );

        if (this.isCasting()) {
          this.castSession.endSession(true);
        }
      }
    }

    this.videoElement?.pause();

    super.disconnectedCallback();
  }

  willUpdate(changedProperties) {
    if (changedProperties.has("playlist")) {
      this.totalTime = this.playlist
        .map((m) => m.duration)
        .reduce((t, v) => t + v, 0);
    }

    if (changedProperties.has("mediaIndex")) {
      this.previousTime = this.playlist
        .slice(0, this.mediaIndex)
        .map((m) => m.duration)
        .reduce((t, v) => t + v, 0);
    }
  }

  togglePlayback(event) {
    if (event.button != 0) {
      return;
    }

    if (this.isCasting) {
      this.castController.playOrPause();
    } else if (this.isPlaying) {
      this.videoElement.pause();
    } else {
      this.videoElement.play();
    }
  }

  showOverlay() {
    let overlay = this.renderRoot.querySelector(".overlay");
    overlay.classList.add("visible");

    if (this._overlayTimeout) {
      clearTimeout(this._overlayTimeout);
    }

    this._overlayTimeout = setTimeout(() => {
      this._overlayTimeout = null;
      overlay.classList.remove("visible");
    }, 3000);
  }

  onFullscreenChanged() {
    this.isFullscreen = document.fullscreenElement == this;
  }

  async toggleFullscreen(event) {
    if (event.button != 0) {
      return;
    }

    try {
      this.isFullscreen = !this.isFullscreen;
      if (!this.isFullscreen) {
        await document.exitFullscreen();
      } else {
        await this.requestFullscreen();
      }
    } catch (e) {
      console.error(e);
      this.onFullscreenChanged();
    }
  }

  updateTime(newTime) {
    if (Math.abs(this.lastReportedTime - newTime) > 10) {
      this.lastReportedTime = newTime;

      let updateUrl = document.documentURI.replace("/library/", "/playback/");
      fetch(`${updateUrl}?position=${this.currentTime}`, {
        method: "POST",
      }).catch((e) => {
        console.error(e);
      });
    }

    this.currentTime = newTime;
  }

  onMediaStateChanged() {
    if (!this.videoElement) {
      return;
    }

    this.isPlaying = !(this.videoElement.paused || this.videoElement.ended);
    this.updateTime(this.previousTime + this.videoElement.currentTime);
  }

  onMediaEnded() {
    if (this.mediaIndex < this.playlist.length - 1) {
      this.previousTime =
        this.previousTime + this.playlist[this.mediaIndex].duration;
      this.mediaIndex++;
    } else {
      this.onMediaStateChanged;
    }
  }

  castMediaInfo(media) {
    let url = new URL(media.url, document.documentURI);

    let mediaInfo = new chrome.cast.media.MediaInfo(
      url.toString(),
      media.mimeType
    );

    let metadata;

    if (this.show) {
      metadata = new chrome.cast.media.TvShowMediaMetadata();
      metadata.originalAirdate = this.airDate;
      metadata.episode = this.episode;
      metadata.season = this.season;
      metadata.seriesTitle = this.show;
    } else {
      metadata = new chrome.cast.media.MovieMediaMetadata();
      metadata.releaseDate = this.airDate;
    }

    metadata.images = [new chrome.cast.Image(this.image)];
    metadata.title = this.title;
    mediaInfo.metadata = metadata;

    return mediaInfo;
  }

  async seek(targetTime) {
    if (targetTime < 0 || targetTime >= this.totalTime) {
      return;
    }

    let previousTime = 0;
    let mediaIndex = 0;
    this.currentTime = targetTime;

    while (targetTime >= this.playlist[mediaIndex].duration) {
      mediaIndex++;
      previousTime += this.playlist[mediaIndex].duration;
      targetTime -= this.playlist[mediaIndex].duration;
    }

    this.previousTime = previousTime;

    if (this.isCasting) {
      if (mediaIndex == this.mediaIndex) {
        this.castPlayer.currentTime = targetTime;
        this.castController.seek();
      } else {
        this.mediaIndex = mediaIndex;

        let mediaInfo = this.castMediaInfo(this.playlist[mediaIndex]);
        let loadRequest = new chrome.cast.media.LoadRequest(mediaInfo);
        loadRequest.currentTime = targetTime;
        await this.castSession.loadMedia(loadRequest);
      }
    } else if (mediaIndex == this.mediaIndex) {
      this.videoElement.currentTime = targetTime;
    } else {
      this.mediaIndex = mediaIndex;
    }
  }

  onProgressClicked(event) {
    if (event.button != 0) {
      return;
    }

    let progress = this.renderRoot.querySelector(".progress");
    let { x: elementX, width: elementWidth } = progress.getBoundingClientRect();

    let offset = event.clientX - elementX;

    let targetTime = (this.totalTime * offset) / elementWidth;
    this.seek(targetTime);
  }

  renderBuffers() {
    let ranges = this.videoElement?.buffered;
    if (!ranges) {
      return [];
    }

    let templates = [];
    for (let i = 0; i < ranges.length; i++) {
      let width = (100 * (ranges.end(i) - ranges.start(i))) / this.totalTime;
      let left = (100 * ranges.start(i)) / this.totalTime;

      templates.push(
        html`<div
          class="buffer"
          style="left: ${left}%; width: ${width}%"
        ></div>`
      );
    }

    return templates;
  }

  seekBack(event) {
    if (event.button != 0) {
      return;
    }

    event.stopPropagation();
    this.seek(Math.max(0, this.currentTime - 30));
  }

  seekForward(event) {
    if (event.button != 0) {
      return;
    }

    event.stopPropagation();
    this.seek(this.currentTime + 30);
  }

  renderVideo() {
    if (this.isCasting) {
      return nothing;
    }

    let media = this.playlist[this.mediaIndex];
    let mediaUrl = new URL(media.url, document.documentURI);
    mediaUrl = new URL(mediaUrl.pathname, document.documentURI);

    return keyed(
      mediaUrl.toString(),
      html`
      <video
        ${ref(this.renderedVideo)}
        autoplay
        preload="auto"
        @ended="${this.onMediaEnded}"
        @pause="${this.onMediaStateChanged}"
        @play="${this.onMediaStateChanged}"
        @timeupdate="${this.onMediaStateChanged}"
        @seeked="${this.onMediaStateChanged}"
      >
        <source type="${media.mimeType}" src="${mediaUrl.toString()}"></source>
      </video>`
    );
  }

  render() {
    let toggleIcon = this.isPlaying ? "pause-fill" : "play-fill";
    let fullscreenIcon = this.isFullscreen
      ? "fullscreen-exit"
      : "arrows-fullscreen";

    let playedPercent = (100 * this.currentTime) / this.totalTime;

    return html`
      ${this.renderVideo()}
      <div class="overlay" @mousemove="${this.showOverlay}">
        <div class="main" @click="${this.togglePlayback}">
          <sl-icon-button
            name="skip-backward"
            @click="${this.seekBack}"
          ></sl-icon-button>
          <sl-icon-button
            name="skip-forward"
            @click="${this.seekForward}"
          ></sl-icon-button>
        </div>
        <div class="controls">
          <sl-icon-button
            name="${toggleIcon}"
            @click="${this.togglePlayback}"
          ></sl-icon-button>
          <div class="time start">${formatTime(this.currentTime)}</div>
          <div class="progress" @click="${this.onProgressClicked}">
            ${this.renderBuffers()}
            <div class="played" style="width: ${playedPercent}%"></div>
            <div class="mask"></div>
          </div>
          <div class="time end">
            -${formatTime(this.totalTime - this.currentTime)}
          </div>
          ${this.isCastAvailable
            ? html`<google-cast-launcher></google-cast-launcher>`
            : nothing}
          <sl-icon-button
            name="${fullscreenIcon}"
            @click="${this.toggleFullscreen}"
          ></sl-icon-button>
        </div>
      </div>
    `;
  }
}

customElements.define("video-player", VideoPlayer);
