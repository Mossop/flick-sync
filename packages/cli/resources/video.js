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
    currentTime: { state: true },
    isPlaying: { state: true },
    isFullscreen: { state: true },
    isCastAvailable: { state: true },
  };

  constructor() {
    super();
    this.currentTime = 0;
    this.mediaIndex = 0;
    this.isPlaying = false;
    this.previousTime = 0;
    this.onFullscreenChanged();
    this.isCastAvailable = window.castState;
    this.videoElement = null;

    if (!this.isCastAvailable) {
      document.addEventListener(
        "cast-available",
        () => (this.isCastAvailable = true),
        { once: true }
      );
    }

    this.addEventListener("fullscreenchange", () => this.onFullscreenChanged());
  }

  renderedVideo(element) {
    if (this.videoElement != element) {
      this.videoElement?.pause();
    }

    this.videoElement = element;
  }

  disconnectedCallback() {
    this.videoElement?.pause();

    super.disconnectedCallback();
  }

  willUpdate(changedProperties) {
    if (changedProperties.has("playlist")) {
      this.totalTime = this.playlist
        .map((m) => m.duration)
        .reduce((t, v) => t + v, 0);
    }
  }

  togglePlayback(event) {
    if (event.button != 0) {
      return;
    }

    if (this.isPlaying) {
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

  onMediaStateChanged() {
    if (!this.videoElement) {
      return;
    }

    this.isPlaying = !(this.videoElement.paused || this.videoElement.ended);
    this.currentTime = this.previousTime + this.videoElement.currentTime;
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

    if (mediaIndex == this.mediaIndex) {
      this.videoElement.currentTime = targetTime;
    } else {
      this.previousTime = previousTime;
      this.mediaIndex = mediaIndex;

      while (!(await this.updateComplete)) {}

      this.videoElement.currentTime = targetTime;
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
    let media = this.playlist[this.mediaIndex];

    return keyed(
      media.url,
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
        <source type="${media.mimeType}" src="${media.url}"></source>
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
